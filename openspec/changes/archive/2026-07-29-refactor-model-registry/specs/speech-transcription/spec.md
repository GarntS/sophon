## MODIFIED Requirements

### Requirement: Per-request transcription options
Both transcription methods SHALL accept an `a{sv}` options dictionary recognizing only `language` as a string. An omitted language SHALL use the configured default, and the selected language SHALL be validated against registry metadata for the active model.

#### Scenario: Request language overrides the default
- **WHEN** a client supplies a language supported by the active model
- **THEN** that language is used for the request without reloading the model

#### Scenario: Invalid option is rejected
- **WHEN** options contain an unknown key, a value of the wrong type, or an unsupported language
- **THEN** the method returns `InvalidOptions` without queueing inference

## REMOVED Requirements

### Requirement: Translation request option
**Reason**: Translation is unsupported by the service and is no longer represented in configuration, capabilities, or request options.

**Migration**: Remove the `translate` key from D-Bus transcription requests and perform translation in a separate service if required.
