## MODIFIED Requirements

### Requirement: File transcription
The `TranscribeFile` method SHALL accept an absolute path to a regular WAV file and an options dictionary, SHALL perform one transcription operation, and SHALL return the final transcribed text in the method reply. Regular-file type, encoded size, and WAV content SHALL be evaluated from the same opened filesystem object. Opening a non-regular object SHALL NOT wait for that object's data source.

#### Scenario: Valid file is transcribed
- **WHEN** a client calls `TranscribeFile` with an accessible absolute path containing supported WAV audio and valid options while the model is ready
- **THEN** the method returns the transcription as a string

#### Scenario: Relative file path is rejected
- **WHEN** a client calls `TranscribeFile` with a relative path
- **THEN** the method returns an `InvalidAudio` error without queueing inference

#### Scenario: Non-regular file is rejected
- **WHEN** the path identifies an object other than a regular file
- **THEN** the method returns an `InvalidAudio` error without waiting for a FIFO writer or queueing inference

#### Scenario: Path changes after opening
- **WHEN** an accepted pathname is replaced after Sophon opens it but before validation or parsing completes
- **THEN** regular-file validation, encoded-size enforcement, and WAV parsing all apply to the originally opened object rather than resolving the pathname again

#### Scenario: Symlink resolves to a regular file
- **WHEN** an absolute input path is a symlink whose target is an accessible regular WAV file
- **THEN** Sophon validates and transcribes the opened target under the same size, duration, and WAV rules as a direct path
