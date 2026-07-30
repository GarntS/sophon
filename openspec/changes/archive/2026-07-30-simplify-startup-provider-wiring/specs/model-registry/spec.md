## MODIFIED Requirements

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
