## Requirements

### Requirement: Package-defined model catalog
Sophon SHALL load exactly one strict, package-installed, read-only `model_registry.yaml` at daemon startup. The catalog SHALL identify models by provider and model name and SHALL define each model's loader kind, revision metadata, supported languages, and mapping of semantic file roles to relative path, HTTPS URL, SHA-256 digest, and expected nonzero size. Each manifest SHALL contain exactly the semantic roles required by its loader kind: Parakeet requires `encoder`, `decoder_joint`, `nemo`, and `vocabulary`; Canary requires `encoder`, `decoder`, `nemo`, and `vocabulary`; Kokoro requires `model` and `voices`; Base, CustomVoice, and VoiceDesign each require `talker` and `codec`.

#### Scenario: Valid package catalog loads
- **WHEN** every installed model contains valid provider, model, metadata, files, and the exact role set for its loader kind
- **THEN** every entry is available for provider/model lookup for the daemon lifetime

#### Scenario: Package catalog data is invalid
- **WHEN** the registry is missing, unreadable, malformed, contains unknown fields or kinds, unsafe paths, invalid URLs or digests, duplicate identities, or invalid file metadata
- **THEN** provider initialization fails terminally without downloading any model

#### Scenario: Loader role set is invalid
- **WHEN** any model manifest is missing a role required by its loader kind, contains an extra role, or substitutes a role belonging to another loader kind
- **THEN** package-catalog validation fails before model resolution, network access, or native loader invocation

### Requirement: Singleton startup-only registry
Sophon SHALL initialize one process-global model registry and SHALL NOT reload or modify its catalog during the daemon lifetime. Provider and model name together SHALL form model identity; revision SHALL NOT be part of identity.

#### Scenario: Registry file changes after startup
- **WHEN** the installed YAML changes while the daemon is running
- **THEN** lookups continue using the startup snapshot until restart

#### Scenario: Same model name exists under two providers
- **WHEN** two providers define the same model name
- **THEN** each model remains independently addressable by its provider/model pair

### Requirement: Role-keyed verified model resolution
The registry SHALL resolve a known provider/model pair to a complete mapping from semantic file role to verified filesystem path. Every returned path SHALL identify a regular file matching its declared size and SHA-256; consumers SHALL validate the roles they require before loading.

#### Scenario: Composite model resolves
- **WHEN** every required blob for a Qwen model verifies
- **THEN** resolution returns distinct `talker` and `codec` role paths

#### Scenario: Consumer requires an absent role
- **WHEN** a provider loader does not receive every semantic role it requires
- **THEN** provider initialization fails before invoking the native loader

### Requirement: Automatic single-attempt resolution
The registry SHALL automatically download missing or invalid required artifacts on first resolution, SHALL share one in-flight attempt among concurrent callers, and SHALL memoize success or failure for that model. A failed attempt SHALL be terminal until daemon restart.

#### Scenario: Concurrent callers request an uncached model
- **WHEN** multiple callers resolve the same model during its first download
- **THEN** they share one attempt and receive the same resolved paths or terminal error

#### Scenario: Download fails transiently
- **WHEN** any required artifact cannot be downloaded or verified
- **THEN** the model remains `Failed` and later requests in that daemon process do not retry

#### Scenario: Daemon restarts after failure
- **WHEN** a new daemon process resolves a model that failed previously
- **THEN** it performs a new attempt and reuses any independently verified blobs

### Requirement: Assembled model views
After all required blobs verify, the registry SHALL atomically assemble a model-specific hard-linked view under declared relative paths and SHALL return role paths within that view.

#### Scenario: Directory loader consumes a model
- **WHEN** a provider requires files with fixed names in one directory
- **THEN** the common parent of returned role paths contains the complete verified model layout

#### Scenario: Package revision changes without changing identity
- **WHEN** a restarted daemon loads an updated manifest for an existing provider/model pair
- **THEN** the registry validates and atomically replaces an obsolete view while reusing blobs whose identities remain valid
