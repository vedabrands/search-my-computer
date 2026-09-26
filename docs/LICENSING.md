# SearchMyComputer — Offline Licensing Architecture

SearchMyComputer uses an **offline, cryptographically secure licensing model**. The application operates with zero network connectivity and never connects to an activation server or license clearinghouse.

---

## 1. Architectural Overview

The licensing subsystem (`smc-license`) balances two core requirements:
1. **100% Offline Cryptographic Verification**: Customers can purchase a perpetual or subscription license and activate it air-gapped on any machine without internet access.
2. **Local Evaluation Protection**: Non-licensed installations receive a 14-day full-featured evaluation trial protected against local timestamp tampering and clock rollbacks.

```
┌────────────────────────────────────────────────────────────────────────┐
│                        Vendor Build / Sales Server                     │
│                                                                        │
│   Ed25519 Private Key ───► license-tool issue ───► Signed License File │
│   (kept strictly private)                          (JSON format)       │
└────────────────────────────────────────────────────────────────────────┘
                                                         │
                                                         ▼
┌────────────────────────────────────────────────────────────────────────┐
│                     SearchMyComputer Client (100% Offline)             │
│                                                                        │
│   Signed License File                                                  │
│          │                                                             │
│          ▼                                                             │
│   Embedded Public Key ───► Ed25519 Verify ───► Valid: Permanent Unlock │
│                                  │                                     │
│                            Invalid / None                              │
│                                  │                                     │
│                                  ▼                                     │
│                       Evaluation State Machine                         │
│                                  │                                     │
│          ┌───────────────────────┴───────────────────────┐             │
│          ▼                                               ▼             │
│   SQLite meta Table                            System Monotonic Clock  │
│   (trial_start, last_seen)                               │             │
│          │                                               │             │
│          ▼                                               ▼             │
│   HMAC-SHA256 Seal Validation ──────────────► Clock Rollback Checks    │
│          │                                               │             │
│          └───────────────────────┬───────────────────────┘             │
│                                  │                                     │
│                       All Checks Valid & < 14 Days                     │
│                                  │                                     │
│                   ┌──────────────┴──────────────┐                      │
│                   ▼                             ▼                      │
│            Trial Active                  Trial Expired                 │
│         (Full Functionality)        (Restricted to Top 3)              │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Cryptographic License Verification

### 2.1 Asymmetric Ed25519 Digital Signatures
- **Algorithm**: Ed25519 (Edwards-curve Digital Signature Algorithm).
- **Public Key**: Hardcoded 32-byte public key embedded in `smc-license` binary at compile time.
- **Private Key**: Kept offline by the vendor / payment processor; never included in client binaries.

### 2.2 Canonical Signed Payload
To prevent field-injection or JSON serialization discrepancies, signatures are computed over the deterministic canonical string:

$$\text{Canonical Payload} = \text{product\_name} \parallel \text{"|"} \parallel \text{major\_version} \parallel \text{"|"} \parallel \text{customer\_name} \parallel \text{"|"} \parallel \text{license\_id} \parallel \text{"|"} \parallel \text{issued\_epoch} \parallel \text{"|"} \parallel \text{expires\_epoch}$$

Where:
- `product_name`: Always `"SearchMyComputer"`.
- `major_version`: The software major version (e.g. `1`).
- `customer_name`: Name or organization of licensee (e.g. `"Jane Doe"`).
- `license_id`: Unique UUIDv4 identifier.
- `issued_epoch`: UNIX epoch timestamp (seconds) when issued.
- `expires_epoch`: `0` for perpetual lifetime licenses, or UNIX epoch expiration timestamp for term licenses.

### 2.3 License File Format
Licenses are stored in `%LOCALAPPDATA%\SearchMyComputer\license.json`:
```json
{
  "product_name": "SearchMyComputer",
  "major_version": 1,
  "customer_name": "Acme Corp",
  "license_id": "7b8f9e12-34a5-67c8-90d1-e2f3a4b5c6d7",
  "issued_epoch": 1774569600,
  "expires_epoch": 0,
  "signature_hex": "4a72e819b7c6d5e4f3a2b10987654321fedcba9876543210abcdef01234567894a72e819b7c6d5e4f3a2b10987654321fedcba9876543210abcdef0123456789"
}
```

---

## 3. Evaluation & Trial Protection

When no commercial license is present, `smc-license` manages a 14-day evaluation period.

### 3.1 SQLite Metadata HMAC-SHA256 Seal
- On first launch, the application records `trial_start_epoch` and `trial_last_seen_epoch` in the SQLite `meta` table.
- A cryptographic HMAC-SHA256 signature (`trial_tamper_hash`) is generated over `trial_start:last_seen` using an internal secret key and stored alongside the timestamps.
- On every application launch and search execution, the integrity hash is recomputed and validated. If an end-user manually edits the SQLite database, the HMAC validation fails and immediately expires the trial.

### 3.2 Monotonic Clock Rollback Detection
- The client persists `trial_last_seen_epoch` on every successful launch.
- If the current system time $T_{\text{now}}$ is in the past compared to the start time ($T_{\text{now}} < T_{\text{start}}$) or earlier than the last seen time by more than 60 seconds ($T_{\text{now}} < T_{\text{last}} - 60$), clock manipulation is flagged and the evaluation expires immediately.

### 3.3 Graceful Expiration Gating
When an evaluation trial expires:
- **Indexation Continues**: Background crawling, file parsing, and indexing remain fully functional.
- **Search Gating**: Search results are restricted to the **top 3 results** in read-only mode.
- **Banner Notification**: The launcher UI displays an amber trial expiration banner with a direct link to open the license activation modal.

---

## 4. Vendor Licensing CLI (`license-tool`)

The `license-tool` binary provides vendor-side key generation, license issuance, verification, and batch issuing.

### 4.1 Commands

#### Generate Keypair
```powershell
cargo run --bin license-tool -- keygen --out-dir ./keys
```
Outputs `private_key.hex` (keep confidential) and `public_key.hex` (embed in `smc-license`).

#### Issue Perpetual License
```powershell
cargo run --bin license-tool -- issue `
  --private-key ./keys/private_key.hex `
  --customer "Acme Corp" `
  --out ./license_acme.json
```

#### Issue 1-Year Subscription License
```powershell
cargo run --bin license-tool -- issue `
  --private-key ./keys/private_key.hex `
  --customer "John Doe" `
  --days 365 `
  --out ./license_john.json
```

#### Verify a License File
```powershell
cargo run --bin license-tool -- verify `
  --public-key ./keys/public_key.hex `
  --license ./license_acme.json
```

#### Batch Issue Licenses from CSV
```powershell
# customers.csv format: Name,Days (0 for lifetime)
# Alice,0
# Bob,365
cargo run --bin license-tool -- batch `
  --private-key ./keys/private_key.hex `
  --input-csv ./customers.csv `
  --out-dir ./issued_licenses
```
