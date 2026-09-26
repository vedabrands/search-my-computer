export interface ChunkMatchSnippet {
  chunk_id: number;
  snippet: string;
  page?: number;
  section?: string;
  symbol?: string;
  score: number;
}

export interface SearchResult {
  id: number;
  path: string;
  name: string;
  parent_dir: string;
  ext: string;
  size: number;
  mtime: string;
  kind: string;
  score: number;
  match_type: string; // "filename" | "content" | "semantic" | "hybrid"
  snippet?: string;
  page?: number;
  section?: string;
  symbol?: string;
  matches?: ChunkMatchSnippet[];
}

export interface ProjectRecord {
  id: number;
  path: string;
  name: string;
  project_type: string; // "rust" | "typescript" | "python" | "go" | "java" | "csharp" | "git"
  manifest_path?: string;
  readme_summary?: string;
  last_detected_at: string;
}

export interface ParsedQuery {
  raw: string;
  text: string;
  file_types: string[];
  after?: string;
  before?: string;
  location_hints: string[];
  is_project_query: boolean;
  is_screenshot_query: boolean;
  use_ctime: boolean;
}

export interface ScanSnapshot {
  files_seen: number;
  files_indexed: number;
  files_skipped: number;
  files_deleted: number;
  errors: number;
}

export interface JobStatusCounts {
  pending: number;
  running: number;
  done: number;
  failed: number;
}

export interface ImageTagDetails {
  tag: string;
  masked_payload?: string;
  raw_payload?: string;
}

export interface ImageDetailsResponse {
  file_id: number;
  width?: number;
  height?: number;
  format?: string;
  exif_date?: string;
  camera_make?: string;
  camera_model?: string;
  is_screenshot: boolean;
  has_qr: boolean;
  qr_count: number;
  tags: ImageTagDetails[];
  ocr_text?: string;
  thumbnail_path?: string;
}

export interface IndexStatus {
  total_files: number;
  active_files: number;
  is_scanning: boolean;
  indexing_paused: boolean;
  scan_progress: ScanSnapshot;
  job_counts: JobStatusCounts;
  indexed_folders: string[];
  hotkey_registered: boolean;
  has_embedding_model?: boolean;
  embedding_model_id?: string;
  enable_image_indexing?: boolean;
  image_folders?: string[];
  has_vision_models?: boolean;
  wake_word_enabled?: boolean;
  wake_word_phrase?: string;
  is_wake_word_paused?: boolean;
}

export interface AppConfig {
  hotkey: string;
  theme: string;
  indexed_folders: string[];
  exclusions: string[];
  max_file_size: number;
  index_threads: number;
  indexing_paused: boolean;
  enable_image_indexing: boolean;
  image_folders: string[];
  launch_at_login: boolean;
  onboarding_completed: boolean;
  index_documents: boolean;
  index_code: boolean;
  index_spreadsheets: boolean;
  index_archives: boolean;
  battery_policy: string;
  language: string;
  max_ram_mb: number;
  license_file_path?: string;
  wake_word_enabled?: boolean;
  wake_word_phrase?: string;
  wake_word_threshold?: number;
  voice_silence_timeout_secs?: number;
  voice_max_duration_secs?: number;
  pill_position?: "bottom-right" | "bottom-left" | "top-right" | "top-left" | "custom";
  pill_custom_x?: number | null;
  pill_custom_y?: number | null;
}

export type PillPositionMode = "bottom-right" | "bottom-left" | "top-right" | "top-left" | "custom";

export interface PillPositionResponse {
  position: PillPositionMode;
  custom_x?: number | null;
  custom_y?: number | null;
}

export type StatusPillVisualState =
  | "idle"
  | "listening"
  | "searching"
  | "results"
  | "indexing"
  | "paused";

export type AudioCaptureState =
  | { state: "Idle" }
  | { state: "WakeWordArmed" }
  | {
      state: "Listening";
      data: {
        duration_secs: number;
        silence_secs: number;
        max_secs: number;
        level: number;
      };
    }
  | { state: "Transcribing" }
  | { state: "Done"; data: { transcription: string } }
  | { state: "Error"; data: { message: string } };

export type LicenseStatus =
  | { status: "Trial"; days_remaining: number; trial_start_epoch: number }
  | { status: "TrialExpired" }
  | { status: "Licensed"; customer_name: string; license_id: string };

export interface ImportLicenseResult {
  success: boolean;
  status: LicenseStatus;
  message: string;
}

export interface UpdateCheckResult {
  current_version: string;
  release_url: string;
}

export interface DetectedFolder {
  name: string;
  path: string;
  category: string;
  is_sensitive: boolean;
  exists: boolean;
  default_checked: boolean;
}

export interface ExclusionTestResult {
  matches: boolean;
  error?: string;
}

export interface FilePreviewResponse {
  path: string;
  name: string;
  ext: string;
  size: number;
  mtime: string;
  kind: string;
  content_preview?: string;
  line_count?: number;
  is_binary: boolean;
  error?: string;
}

export interface ProblemFileRecord {
  id: number;
  file_id?: number;
  path: string;
  error_kind: string;
  error_message: string;
  attempts: number;
  last_failed_at: string;
  resolved_at?: string;
}

export interface DiagnosticsExportResult {
  file_path: string;
  size_bytes: number;
  summary: string;
}

export interface GovernorStatus {
  battery_percentage?: number;
  is_on_ac: boolean;
  is_low_battery: boolean;
  is_thermal_throttled: boolean;
  user_paused: boolean;
  state: string;
  active_threads: number;
}
