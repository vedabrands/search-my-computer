use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::Utc;
use clap::{Parser, Subcommand};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::Serialize;
use sha2::{Digest, Sha256};
use smc_license::license::{LicenseFile, PRODUCT_NAME, verify_license};
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "license-tool",
    about = "SearchMyComputer license management CLI"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new Ed25519 keypair for license signing
    Keygen {
        /// Output directory for key files
        #[arg(long, default_value = "keys")]
        output: PathBuf,
    },
    /// Issue a signed license for a customer
    Issue {
        /// Path to the private signing key (hex-encoded 32 bytes)
        #[arg(long)]
        key: PathBuf,
        /// Customer name
        #[arg(long)]
        name: String,
        /// Customer email (hashed with SHA-256 before storage)
        #[arg(long)]
        email: String,
        /// Maximum number of devices
        #[arg(long, default_value = "3")]
        max_devices: u32,
        /// Major version this license is valid for
        #[arg(long, default_value = "1")]
        major_version: u32,
        /// Output .lic file path
        #[arg(long)]
        output: PathBuf,
    },
    /// Issue licenses in batch from a CSV file
    Batch {
        /// Path to the private signing key (hex-encoded 32 bytes)
        #[arg(long)]
        key: PathBuf,
        /// CSV file with columns: name, email, max_devices (optional)
        #[arg(long)]
        csv: PathBuf,
        /// Output directory for .lic files
        #[arg(long, default_value = "licenses")]
        output_dir: PathBuf,
        /// Major version
        #[arg(long, default_value = "1")]
        major_version: u32,
    },
    /// Verify a .lic file against the public key
    Verify {
        /// Path to the public key (hex-encoded 32 bytes)
        #[arg(long)]
        key: PathBuf,
        /// Path to the .lic file to verify
        #[arg(long)]
        license: PathBuf,
    },
}

/// The payload subset that the signature covers (sorted keys).
#[derive(Serialize)]
struct LicensePayload {
    customer_name: String,
    email_hash: String,
    issued_at: String,
    license_id: String,
    major_version: u32,
    max_devices: u32,
    product: String,
}

fn hash_email(email: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(email.trim().to_lowercase().as_bytes());
    hex::encode(hasher.finalize())
}

fn generate_license_id() -> String {
    let id = uuid::Uuid::new_v4();
    format!("SMC-{}", id.to_string()[..8].to_uppercase())
}

fn sign_license(
    signing_key: &SigningKey,
    name: &str,
    email: &str,
    max_devices: u32,
    major_version: u32,
) -> LicenseFile {
    let license_id = generate_license_id();
    let email_hash = hash_email(email);
    let issued_at = Utc::now().to_rfc3339();

    let payload = LicensePayload {
        customer_name: name.to_string(),
        email_hash: email_hash.clone(),
        issued_at: issued_at.clone(),
        license_id: license_id.clone(),
        major_version,
        max_devices,
        product: PRODUCT_NAME.to_string(),
    };

    let payload_bytes = serde_json::to_vec(&payload).expect("serialization infallible");
    let signature = signing_key.sign(&payload_bytes);
    let sig_b64 = BASE64.encode(signature.to_bytes());

    LicenseFile {
        license_id,
        email_hash,
        customer_name: name.to_string(),
        product: PRODUCT_NAME.to_string(),
        major_version,
        issued_at,
        max_devices,
        signature: sig_b64,
    }
}

fn load_signing_key(path: &PathBuf) -> SigningKey {
    let hex_str = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read key file {}: {}", path.display(), e));
    let key_bytes =
        hex::decode(hex_str.trim()).unwrap_or_else(|e| panic!("invalid hex in key file: {}", e));
    let key_array: [u8; 32] = key_bytes
        .try_into()
        .unwrap_or_else(|_| panic!("key must be exactly 32 bytes"));
    SigningKey::from_bytes(&key_array)
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Keygen { output } => {
            fs::create_dir_all(&output).expect("failed to create output directory");

            let signing_key = SigningKey::generate(&mut OsRng);
            let verifying_key: VerifyingKey = (&signing_key).into();

            let private_path = output.join("private_key.hex");
            let public_path = output.join("public_key.hex");
            let public_rust_path = output.join("public_key_rust.txt");

            fs::write(&private_path, hex::encode(signing_key.to_bytes()))
                .expect("failed to write private key");
            fs::write(&public_path, hex::encode(verifying_key.to_bytes()))
                .expect("failed to write public key");

            // Also emit a Rust array literal for embedding
            let bytes = verifying_key.to_bytes();
            let rust_literal = format!(
                "pub const EMBEDDED_PUBLIC_KEY: [u8; 32] = [\n    {}\n];",
                bytes
                    .chunks(8)
                    .map(|chunk| {
                        chunk
                            .iter()
                            .map(|b| format!("0x{:02x}", b))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .collect::<Vec<_>>()
                    .join(",\n    ")
            );
            fs::write(&public_rust_path, rust_literal).expect("failed to write Rust key literal");

            println!("✅ Keypair generated:");
            println!("   Private key: {}", private_path.display());
            println!("   Public key:  {}", public_path.display());
            println!("   Rust embed:  {}", public_rust_path.display());
            println!("\n⚠️  Keep private_key.hex SECRET. Never commit it to git.");
        }

        Commands::Issue {
            key,
            name,
            email,
            max_devices,
            major_version,
            output,
        } => {
            let signing_key = load_signing_key(&key);
            let license = sign_license(&signing_key, &name, &email, max_devices, major_version);
            let json = serde_json::to_string_pretty(&license).unwrap();

            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).ok();
            }
            fs::write(&output, &json).expect("failed to write license file");

            println!("✅ License issued:");
            println!("   ID:       {}", license.license_id);
            println!("   Customer: {}", license.customer_name);
            println!("   Devices:  {}", license.max_devices);
            println!("   File:     {}", output.display());
        }

        Commands::Batch {
            key,
            csv,
            output_dir,
            major_version,
        } => {
            let signing_key = load_signing_key(&key);
            fs::create_dir_all(&output_dir).expect("failed to create output directory");

            let mut reader = csv::Reader::from_path(&csv)
                .unwrap_or_else(|e| panic!("failed to read CSV {}: {}", csv.display(), e));

            let mut count = 0u32;
            for result in reader.records() {
                let record = result.expect("invalid CSV row");
                let name = record.get(0).expect("missing name column").trim();
                let email = record.get(1).expect("missing email column").trim();
                let max_devices: u32 = record
                    .get(2)
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(3);

                let license = sign_license(&signing_key, name, email, max_devices, major_version);
                let filename = format!("{}.lic", license.license_id);
                let path = output_dir.join(&filename);
                let json = serde_json::to_string_pretty(&license).unwrap();
                fs::write(&path, &json).expect("failed to write license");

                println!("  {} → {}", name, filename);
                count += 1;
            }
            println!("✅ Issued {} licenses to {}", count, output_dir.display());
        }

        Commands::Verify { key, license } => {
            let pub_hex = fs::read_to_string(&key)
                .unwrap_or_else(|e| panic!("failed to read public key: {}", e));
            let pub_bytes = hex::decode(pub_hex.trim())
                .unwrap_or_else(|e| panic!("invalid hex in public key: {}", e));
            let pub_array: [u8; 32] = pub_bytes
                .try_into()
                .unwrap_or_else(|_| panic!("public key must be 32 bytes"));

            let license_json = fs::read_to_string(&license)
                .unwrap_or_else(|e| panic!("failed to read license file: {}", e));

            match verify_license(&license_json, &pub_array, PRODUCT_NAME, 1) {
                Ok(lic) => {
                    println!("✅ License is VALID");
                    println!("   ID:       {}", lic.license_id);
                    println!("   Customer: {}", lic.customer_name);
                    println!("   Product:  {}", lic.product);
                    println!("   Version:  {}", lic.major_version);
                    println!("   Devices:  {}", lic.max_devices);
                    println!("   Issued:   {}", lic.issued_at);
                }
                Err(e) => {
                    eprintln!("❌ License verification FAILED: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}
