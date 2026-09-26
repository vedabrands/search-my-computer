use smc_core::config::AppConfig;
use smc_core::db::Database;
use smc_core::governor::ResourceGovernor;
use smc_core::jobs::JobQueue;
use smc_core::scanner::{ScanProgress, Scanner};
use smc_core::watcher::FileWatcher;
use smc_embed::embedder::{DEFAULT_MODEL_ID, EmbeddingModelConfig, OnnxEmbedder};
use smc_embed::vector_index::SqliteVectorIndex;
use smc_extract::{ExtractionLimits, ExtractorRegistry};
use smc_license::TrialManager;
use smc_license::license::{
    EMBEDDED_PUBLIC_KEY, LicenseStatus, MAJOR_VERSION, PRODUCT_NAME, verify_license,
};
use smc_stt::capture::{AudioCaptureManager, AudioStream, CaptureConfig};
use smc_stt::transcriber::{WhisperConfig, WhisperTranscriber};
use smc_stt::wakeword::{WakeWordConfig, WakeWordEngine};
use smc_vision::clip::{ClipConfig, ClipEngine};
use smc_vision::ocr::{OcrConfig, OcrEngine};
use smc_vision::pipeline::VisionPipeline;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tracing::{info, warn};

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub config: Arc<parking_lot::Mutex<AppConfig>>,
    pub job_queue: Arc<JobQueue>,
    pub governor: Arc<ResourceGovernor>,
    pub watcher: Arc<parking_lot::Mutex<FileWatcher>>,
    pub scan_progress: Arc<ScanProgress>,
    pub is_scanning: Arc<AtomicBool>,
    pub hotkey_registered: Arc<AtomicBool>,
    pub extractor_registry: Arc<ExtractorRegistry>,
    pub extraction_limits: ExtractionLimits,
    pub embedder: Option<Arc<OnnxEmbedder>>,
    pub vector_index: Arc<SqliteVectorIndex>,
    pub vision_pipeline: Arc<VisionPipeline>,
    pub license_status: Arc<parking_lot::Mutex<LicenseStatus>>,
    pub voice_manager: Arc<AudioCaptureManager>,
    pub mic_stream: Arc<parking_lot::Mutex<Option<AudioStream>>>,
}

fn resolve_models_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        candidates.push(parent.join("models"));
        candidates.push(parent.join("resources").join("models"));
        candidates.push(parent.join("..").join("models"));
        candidates.push(parent.join("..").join("..").join("models"));
    }

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("models"));
        candidates.push(cwd.join("..").join("models"));
        candidates.push(cwd.join("..").join("..").join("models"));
    }

    candidates.push(PathBuf::from("models"));
    candidates.push(PathBuf::from("../models"));
    candidates.push(PathBuf::from("../../models"));

    for candidate in candidates {
        let model_file = candidate
            .join(DEFAULT_MODEL_ID)
            .join("model_quantized.onnx");
        let tokenizer_file = candidate.join(DEFAULT_MODEL_ID).join("tokenizer.json");
        if model_file.exists() && tokenizer_file.exists() {
            return Some(candidate);
        }
    }

    None
}

impl AppState {
    pub fn new(db: Database, config: AppConfig) -> Self {
        let config_arc = Arc::new(parking_lot::Mutex::new(config.clone()));
        let governor = Arc::new(ResourceGovernor::new_default());
        let job_queue = Arc::new(
            JobQueue::new(db.clone(), config.index_threads).with_governor(governor.clone()),
        );
        let watcher = Arc::new(parking_lot::Mutex::new(
            FileWatcher::new(db.clone(), config_arc.clone()).unwrap(),
        ));
        let scanner = Scanner::new(db.clone(), &config).unwrap();
        let scan_progress = scanner.progress();
        let extractor_registry = Arc::new(ExtractorRegistry::new());
        let extraction_limits = ExtractionLimits {
            max_bytes: config.max_file_size as usize,
            max_pages: 1000,
            timeout_ms: 10_000,
        };

        let vector_index = Arc::new(SqliteVectorIndex::new(db.clone()));

        // Evaluate license/trial status on startup.
        let license_status = {
            let mut status = None;

            // First, check if the user has an imported license file.
            if let Some(ref lic_path) = config.license_file_path {
                if let Ok(json) = std::fs::read_to_string(lic_path) {
                    match verify_license(&json, &EMBEDDED_PUBLIC_KEY, PRODUCT_NAME, MAJOR_VERSION) {
                        Ok(lic) => {
                            info!(
                                license_id = %lic.license_id,
                                customer = %lic.customer_name,
                                "valid license loaded"
                            );
                            status = Some(LicenseStatus::Licensed {
                                customer_name: lic.customer_name,
                                license_id: lic.license_id,
                            });
                        }
                        Err(e) => {
                            warn!(path = %lic_path, error = %e, "saved license file failed verification, falling back to trial");
                        }
                    }
                } else {
                    warn!(path = %lic_path, "saved license file not readable, falling back to trial");
                }
            }

            // Fall back to trial evaluation.
            if status.is_none() {
                let writer = db.writer();
                status = Some(TrialManager::evaluate(&writer).unwrap_or_else(|e| {
                    warn!(error = %e, "trial evaluation failed, defaulting to expired");
                    LicenseStatus::TrialExpired
                }));
            }

            let s = status.unwrap_or(LicenseStatus::TrialExpired);
            info!(license_status = ?s, "license status resolved");
            Arc::new(parking_lot::Mutex::new(s))
        };

        let models_dir_resolved = resolve_models_dir();
        let models_dir_buf = models_dir_resolved
            .clone()
            .unwrap_or_else(|| PathBuf::from("models"));
        let thumbnail_dir = AppConfig::data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("thumbnails");
        let ocr_engine = OcrEngine::new(OcrConfig::new(&models_dir_buf));
        let clip_engine = ClipEngine::new(ClipConfig::new(&models_dir_buf));
        let vision_pipeline = Arc::new(VisionPipeline::new(thumbnail_dir, ocr_engine, clip_engine));

        let embedder = if let Some(models_dir) = models_dir_resolved {
            let model_cfg = EmbeddingModelConfig::default_bge_small(&models_dir);
            info!(
                model_path = %model_cfg.model_path.display(),
                tokenizer_path = %model_cfg.tokenizer_path.display(),
                "embedding model found, configuring ONNX embedder"
            );
            Some(Arc::new(OnnxEmbedder::new(model_cfg)))
        } else {
            warn!(
                "no valid ONNX embedding model found in models directory; semantic search will be disabled (graceful degradation)"
            );
            None
        };

        let ww_model_path = models_dir_buf.join("wake_words").join("kira.onnx");
        let ww_emb_path = models_dir_buf
            .join("wake_words")
            .join("embedding_model.onnx");
        let ww_config = WakeWordConfig {
            target_phrase: config.wake_word_phrase.clone(),
            model_path: ww_model_path,
            embedding_model_path: Some(ww_emb_path),
            threshold: config.wake_word_threshold,
            num_threads: 1,
        };
        let ww_engine = Arc::new(WakeWordEngine::new(ww_config));

        let whisper_dir = models_dir_buf.join("whisper");
        let whisper_config = WhisperConfig {
            encoder_path: whisper_dir.join("whisper-tiny-encoder.onnx"),
            decoder_path: whisper_dir.join("whisper-tiny-decoder.onnx"),
            tokenizer_path: whisper_dir.join("tokenizer.json"),
            num_threads: 2,
            idle_unload_secs: 300,
        };
        let transcriber = Arc::new(WhisperTranscriber::new(whisper_config));

        let capture_config = CaptureConfig {
            wake_word_enabled: config.wake_word_enabled,
            silence_timeout_secs: config.voice_silence_timeout_secs,
            max_duration_secs: config.voice_max_duration_secs,
            wake_word_phrase: config.wake_word_phrase.clone(),
        };
        let voice_manager = Arc::new(AudioCaptureManager::new(
            capture_config,
            ww_engine,
            transcriber,
        ));

        let mic_stream = Arc::new(parking_lot::Mutex::new(None));
        if config.wake_word_enabled {
            match AudioCaptureManager::spawn_mic_stream(voice_manager.clone()) {
                Ok(stream) => {
                    *mic_stream.lock() = Some(stream);
                    info!("background microphone stream initialized for wake-word listening");
                }
                Err(e) => {
                    warn!(error = %e, "could not initialize background microphone stream on startup");
                }
            }
        }

        Self {
            db,
            config: config_arc,
            job_queue,
            governor,
            watcher,
            scan_progress,
            is_scanning: Arc::new(AtomicBool::new(false)),
            hotkey_registered: Arc::new(AtomicBool::new(false)),
            extractor_registry,
            extraction_limits,
            embedder,
            vector_index,
            vision_pipeline,
            license_status,
            voice_manager,
            mic_stream,
        }
    }
}
