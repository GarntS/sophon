## Requirements

### Requirement: Content-addressed shared artifacts
Sophon SHALL store curated model files beneath a shared `artifacts` cache keyed by SHA-256 digest, and model definitions SHALL resolve their required files by verified artifact identity so identical files, including the Qwen codec, occupy one cache entry.

#### Scenario: Two models use the same codec
- **WHEN** two curated Qwen model definitions reference the same codec digest
- **THEN** both resolve the same verified codec artifact without downloading or storing a second copy

#### Scenario: Cached artifact is corrupt
- **WHEN** an artifact exists but its size or SHA-256 does not match its manifest
- **THEN** no model may load it and acquisition treats it as unavailable

### Requirement: Atomic per-artifact acquisition
Sophon SHALL coordinate acquisition by artifact digest, stream each missing artifact into temporary storage, verify its exact expected size and SHA-256, flush it, and atomically publish it. Failed or interrupted acquisition SHALL NOT make a partial artifact loadable and SHALL NOT invalidate other verified artifacts.

#### Scenario: Concurrent models require one artifact
- **WHEN** concurrent acquisition paths require the same missing digest
- **THEN** a per-digest lock ensures at most one publication and all successful waiters reuse the verified result

#### Scenario: Talker download fails after codec completion
- **WHEN** the shared codec is verified but its model's talker download later fails
- **THEN** the codec remains valid and reusable while the incomplete talker remains unavailable

#### Scenario: Download digest mismatches
- **WHEN** streamed bytes do not match the artifact's expected size or digest
- **THEN** acquisition fails, removes or quarantines temporary data, and does not publish the artifact

### Requirement: Byte-level acquisition progress
Model acquisition progress SHALL represent the verified or downloaded bytes for all required artifacts divided by their total manifest size and SHALL update as network chunks are persisted.

#### Scenario: Large artifact is downloading
- **WHEN** only part of a large required artifact has arrived
- **THEN** lifecycle progress advances proportionally before that file completes

#### Scenario: Shared artifact is already valid
- **WHEN** acquisition begins with one required artifact already verified
- **THEN** its full expected size immediately contributes to progress and only missing bytes are downloaded

#### Scenario: Acquisition completes
- **WHEN** every required artifact is verified and published
- **THEN** progress reaches 1.0 before model loading begins