## MODIFIED Requirements

### Requirement: XDG model cache override
The default model cache SHALL be `sophon/models` beneath `$XDG_CACHE_HOME` when set and `~/.cache/sophon/models` otherwise. A configured cache directory SHALL be absolute, SHALL be either nonexistent or an existing directory, and SHALL override the default as the one shared root for registry artifact acquisition, verified model views, and provider cache data. An invalid configured root SHALL fail strict configuration before model resolution and SHALL NOT silently select the XDG default.

#### Scenario: No cache override is configured
- **WHEN** model acquisition needs a cache and configuration omits `cache_dir`
- **THEN** the service uses the XDG-derived model cache path as the shared registry root

#### Scenario: Cache override is configured
- **WHEN** configuration supplies a valid absolute cache directory
- **THEN** automatic model acquisition, lookup, assembled views, and provider cache data use that directory

#### Scenario: Nonexistent absolute cache override is configured
- **WHEN** configuration supplies an absolute cache path that does not yet exist
- **THEN** configuration accepts it and model acquisition creates the required cache directories on first use

#### Scenario: Invalid cache override is configured
- **WHEN** configuration supplies a relative cache path or a path that exists as a non-directory
- **THEN** strict configuration fails before registry resolution without falling back to the XDG cache
