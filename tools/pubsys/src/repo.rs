//! The repo module owns the 'repo' subcommand and controls the process of building a repository.

pub(crate) mod check_expirations;
pub(crate) mod refresh_repo;
pub(crate) mod validate_repo;

use crate::{friendly_version, Args};
use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use log::{debug, info, trace, warn};
use parse_datetime::parse_datetime;
use pubsys_config::{
    InfraConfig, KMSKeyConfig, RepoConfig, RepoExpirationPolicy, SigningKeyConfig,
};
use rusoto_core::Region;
use rusoto_kms::KmsClient;
use semver::Version;
use snafu::{ensure, OptionExt, ResultExt};
use std::convert::TryInto;
use std::fs::{self, File};
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use structopt::StructOpt;
use tempfile::NamedTempFile;
use tough::{
    editor::signed::PathExists,
    editor::RepositoryEditor,
    key_source::{KeySource, LocalKeySource},
    schema::Target,
    RepositoryLoader, TransportErrorKind,
};
use tough_kms::{KmsKeySource, KmsSigningAlgorithm};
use tough_ssm::SsmKeySource;
use update_metadata::{Images, Manifest, Release, UpdateWaves};
use url::Url;

lazy_static! {
    static ref DEFAULT_START_TIME: DateTime<Utc> = Utc::now();
}

/// Builds Bottlerocket repos using latest build artifacts
#[derive(Debug, StructOpt)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
pub(crate) struct RepoArgs {
    // Metadata about the update
    #[structopt(long)]
    /// Use this named repo infrastructure from Infra.toml
    repo: String,
    #[structopt(long)]
    /// The architecture of the repo and the update being added
    arch: String,
    #[structopt(long, parse(try_from_str=friendly_version))]
    /// The version of the update being added
    version: Version,
    #[structopt(long)]
    /// The variant of the update being added
    variant: String,

    // The images to add in this update
    #[structopt(long, parse(from_os_str))]
    /// Path to the image containing the boot partition
    boot_image: PathBuf,
    #[structopt(long, parse(from_os_str))]
    /// Path to the image containing the root partition
    root_image: PathBuf,
    #[structopt(long, parse(from_os_str))]
    /// Path to the image containing the verity hashes
    hash_image: PathBuf,

    // Optionally add other files to the repo
    #[structopt(long = "link-target", parse(from_os_str))]
    /// Optional paths to add as targets and symlink into repo
    link_targets: Vec<PathBuf>,
    #[structopt(long = "copy-target", parse(from_os_str))]
    /// Optional paths to add as targets and copy into repo
    copy_targets: Vec<PathBuf>,

    // Policies that pubsys interprets to set repo parameters
    #[structopt(long, parse(from_os_str))]
    /// Path to file that defines when repo metadata should expire
    repo_expiration_policy_path: PathBuf,

    // Configuration that pubsys passes on to other tools
    #[structopt(long, parse(from_os_str))]
    /// Path to Release.toml
    release_config_path: PathBuf,
    #[structopt(long, parse(from_os_str))]
    /// Path to file that defines when this update will become available
    wave_policy_path: PathBuf,
    #[structopt(long, parse(from_os_str))]
    /// Path to root.json for this repo
    root_role_path: PathBuf,
    #[structopt(long, parse(from_os_str))]
    /// If we generated a local key, we'll find it here; used if Infra.toml has no key defined
    default_key_path: PathBuf,

    #[structopt(long, parse(try_from_str = parse_datetime))]
    /// When the waves and expiration timer will start; RFC3339 date or "in X hours/days/weeks"
    release_start_time: Option<DateTime<Utc>>,

    #[structopt(long, parse(from_os_str))]
    /// Where to store the created repo
    outdir: PathBuf,
}

/// Adds update, migrations, and waves to the Manifest
fn update_manifest(repo_args: &RepoArgs, manifest: &mut Manifest) -> Result<()> {
    // Add update   =^..^=   =^..^=   =^..^=   =^..^=

    let filename = |path: &PathBuf| -> Result<String> {
        Ok(path
            .file_name()
            .context(error::InvalidImagePath { path })?
            .to_str()
            .context(error::NonUtf8Path { path })?
            .to_string())
    };

    let images = Images {
        boot: filename(&repo_args.boot_image)?,
        root: filename(&repo_args.root_image)?,
        hash: filename(&repo_args.hash_image)?,
    };

    info!(
        "Adding update to manifest for version: {}, arch: {}, variant: {}",
        repo_args.version, repo_args.arch, repo_args.variant
    );
    manifest
        .add_update(
            repo_args.version.clone(),
            None,
            repo_args.arch.clone(),
            repo_args.variant.clone(),
            images,
        )
        .context(error::AddUpdate)?;

    // Add migrations   =^..^=   =^..^=   =^..^=   =^..^=

    info!(
        "Using release config from path: {}",
        repo_args.release_config_path.display()
    );
    let release =
        Release::from_path(&repo_args.release_config_path).context(error::UpdateMetadataRead {
            path: &repo_args.release_config_path,
        })?;
    trace!(
        "Adding migrations to manifest for versions: {:#?}",
        release
            .migrations
            .keys()
            .map(|(from, to)| format!("({}, {})", from, to))
            .collect::<Vec<String>>()
    );
    // Replace the manifest 'migrations' section with the new data
    manifest.migrations = release.migrations;

    // Add update waves   =^..^=   =^..^=   =^..^=   =^..^=

    let wave_start_time = repo_args.release_start_time.unwrap_or(*DEFAULT_START_TIME);
    info!(
        "Using wave policy from path: {}",
        repo_args.wave_policy_path.display()
    );
    info!(
        "Offsets from that file will be added to the release start time of: {}",
        wave_start_time
    );
    let waves =
        UpdateWaves::from_path(&repo_args.wave_policy_path).context(error::UpdateMetadataRead {
            path: &repo_args.wave_policy_path,
        })?;
    manifest
        .set_waves(
            repo_args.variant.clone(),
            repo_args.arch.clone(),
            repo_args.version.clone(),
            wave_start_time,
            &waves,
        )
        .context(error::SetWaves {
            wave_policy_path: &repo_args.wave_policy_path,
        })?;

    Ok(())
}

/// Set expirations of all non-root role metadata based on a given `RepoExpirationPolicy` and an
/// expiration start time
fn set_expirations(
    editor: &mut RepositoryEditor,
    expiration_policy: &RepoExpirationPolicy,
    expiration_start_time: DateTime<Utc>,
) -> Result<()> {
    let snapshot_expiration = expiration_start_time + expiration_policy.snapshot_expiration;
    let targets_expiration = expiration_start_time + expiration_policy.targets_expiration;
    let timestamp_expiration = expiration_start_time + expiration_policy.timestamp_expiration;
    info!(
        "Setting non-root metadata expiration times:\n\tsnapshot:  {}\n\ttargets:   {}\n\ttimestamp: {}",
        snapshot_expiration, targets_expiration, timestamp_expiration
    );
    editor
        .snapshot_expires(snapshot_expiration)
        .targets_expires(targets_expiration)
        .context(error::SetTargetsExpiration {
            expiration: targets_expiration,
        })?
        .timestamp_expires(timestamp_expiration);

    Ok(())
}

/// Set versions of all role metadata; the version will be the UNIX timestamp of the current time.
fn set_versions(editor: &mut RepositoryEditor) -> Result<()> {
    let seconds = Utc::now().timestamp();
    let unsigned_seconds = seconds.try_into().expect("System clock before 1970??");
    let version = NonZeroU64::new(unsigned_seconds).expect("System clock exactly 1970??");
    debug!("Repo version: {}", version);
    editor
        .snapshot_version(version)
        .targets_version(version)
        .context(error::SetTargetsVersion { version })?
        .timestamp_version(version);

    Ok(())
}

/// Adds targets, expirations, and version to the RepositoryEditor
fn update_editor<'a, P>(
    repo_args: &'a RepoArgs,
    editor: &mut RepositoryEditor,
    targets: impl Iterator<Item = &'a PathBuf>,
    manifest_path: P,
) -> Result<()>
where
    P: AsRef<Path>,
{
    // Add targets   =^..^=   =^..^=   =^..^=   =^..^=

    for target_path in targets {
        debug!("Adding target from path: {}", target_path.display());
        editor
            .add_target_path(&target_path)
            .context(error::AddTarget { path: &target_path })?;
    }

    let manifest_target = Target::from_path(&manifest_path).context(error::BuildTarget {
        path: manifest_path.as_ref(),
    })?;
    debug!("Adding target for manifest.json");
    editor
        .add_target("manifest.json", manifest_target)
        .context(error::AddTarget {
            path: "manifest.json",
        })?;

    // Add expirations   =^..^=   =^..^=   =^..^=   =^..^=

    info!(
        "Using repo expiration policy from path: {}",
        repo_args.repo_expiration_policy_path.display()
    );
    let expiration = RepoExpirationPolicy::from_path(&repo_args.repo_expiration_policy_path)
        .context(error::Config)?;

    let expiration_start_time = repo_args.release_start_time.unwrap_or(*DEFAULT_START_TIME);
    let snapshot_expiration = expiration_start_time + expiration.snapshot_expiration;
    let targets_expiration = expiration_start_time + expiration.targets_expiration;
    let timestamp_expiration = expiration_start_time + expiration.timestamp_expiration;
    info!(
        "Repo expiration times:\n\tsnapshot:  {}\n\ttargets:   {}\n\ttimestamp: {}",
        snapshot_expiration, targets_expiration, timestamp_expiration
    );
    editor
        .snapshot_expires(snapshot_expiration)
        .targets_expires(targets_expiration)
        .context(error::SetTargetsExpiration {
            expiration: targets_expiration,
        })?
        .timestamp_expires(timestamp_expiration);

    // Add version   =^..^=   =^..^=   =^..^=   =^..^=

    let seconds = Utc::now().timestamp();
    let unsigned_seconds = seconds.try_into().expect("System clock before 1970??");
    let version = NonZeroU64::new(unsigned_seconds).expect("System clock exactly 1970??");
    debug!("Repo version: {}", version);
    editor
        .snapshot_version(version)
        .targets_version(version)
        .context(error::SetTargetsVersion { version })?
        .timestamp_version(version);

    Ok(())
}

/// If the infra config has a repo section defined for the given repo, and it has metadata base and
/// targets URLs defined, returns those URLs, otherwise None.
fn repo_urls<'a>(
    repo_config: &'a RepoConfig,
    variant: &str,
    arch: &str,
) -> Result<Option<(Url, &'a Url)>> {
    // Check if both URLs are set
    if let Some(metadata_base_url) = repo_config.metadata_base_url.as_ref() {
        if let Some(targets_url) = repo_config.targets_url.as_ref() {
            let base_slash = if metadata_base_url.as_str().ends_with('/') {
                ""
            } else {
                "/"
            };
            let metadata_url_str =
                format!("{}{}{}/{}", metadata_base_url, base_slash, variant, arch);
            let metadata_url = Url::parse(&metadata_url_str).context(error::ParseUrl {
                input: &metadata_url_str,
            })?;

            debug!("Using metadata url: {}", metadata_url);
            return Ok(Some((metadata_url, targets_url)));
        }
    }

    Ok(None)
}

/// Builds an editor and manifest; will start from an existing repo if one is specified in the
/// configuration.  Returns Err if we fail to read from the repo.  Returns Ok(None) if we detect
/// that the repo does not exist.
fn load_editor_and_manifest<'a, P>(
    root_role_path: P,
    metadata_url: &'a Url,
    targets_url: &'a Url,
) -> Result<Option<(RepositoryEditor, Manifest)>>
where
    P: AsRef<Path>,
{
    let root_role_path = root_role_path.as_ref();

    // Try to load the repo...
    let repo_load_result = RepositoryLoader::new(
        File::open(root_role_path).context(error::File {
            path: root_role_path,
        })?,
        metadata_url.clone(),
        targets_url.clone(),
    )
    .load();

    match repo_load_result {
        // If we load it successfully, build an editor and manifest from it.
        Ok(repo) => {
            let target = "manifest.json";
            let target = target
                .try_into()
                .context(error::ParseTargetName { target })?;
            let reader = repo
                .read_target(&target)
                .context(error::ReadTarget {
                    target: target.raw(),
                })?
                .with_context(|| error::NoManifest {
                    metadata_url: metadata_url.clone(),
                })?;
            let manifest = serde_json::from_reader(reader).context(error::InvalidJson {
                path: "manifest.json",
            })?;

            let editor =
                RepositoryEditor::from_repo(root_role_path, repo).context(error::EditorFromRepo)?;

            Ok(Some((editor, manifest)))
        }
        // If we fail to load, but we only failed because the repo doesn't exist yet, then start
        // fresh by signalling that there is no known repo.  Otherwise, fail hard.
        Err(e) => {
            if is_file_not_found_error(&e) {
                Ok(None)
            } else {
                Err(e).with_context(|| error::RepoLoad {
                    metadata_base_url: metadata_url.clone(),
                })
            }
        }
    }
}

/// Inspects the `tough` error to see if it is a `Transport` error, and if so, is it `FileNotFound`.
fn is_file_not_found_error(e: &tough::error::Error) -> bool {
    if let tough::error::Error::Transport { source, .. } = e {
        matches!(source.kind(), TransportErrorKind::FileNotFound)
    } else {
        false
    }
}

/// Gets the corresponding `KeySource` according to the signing key config from Infra.toml
fn get_signing_key_source(signing_key_config: &SigningKeyConfig) -> Result<Box<dyn KeySource>> {
    match signing_key_config {
        SigningKeyConfig::file { path } => Ok(Box::new(LocalKeySource { path: path.clone() })),
        SigningKeyConfig::kms { key_id, config, .. } => Ok(Box::new(KmsKeySource {
            profile: None,
            key_id: key_id
                .clone()
                .context(error::MissingConfig { missing: "key_id" })?,
            client: {
                let key_id_val = key_id
                    .clone()
                    .context(error::MissingConfig { missing: "key_id" })?;
                config
                    .as_ref()
                    .map_or(Ok(None), |config_val| get_client(&config_val, &key_id_val))?
            },
            signing_algorithm: KmsSigningAlgorithm::RsassaPssSha256,
        })),
        SigningKeyConfig::ssm { parameter } => Ok(Box::new(SsmKeySource {
            profile: None,
            parameter_name: parameter.clone(),
            key_id: None,
        })),
    }
}

/// Helper function that generations KMSClient with region (or None) given config containing available keys
fn get_client(config: &KMSKeyConfig, key_id: &String) -> Result<Option<KmsClient>> {
    if let Some(region) = config.available_keys.get(key_id) {
        Ok(Some(KmsClient::new(
            Region::from_str(region).context(error::ParseRegion { what: region })?,
        )))
    } else {
        Ok(None)
    }
}

/// Common entrypoint from main()
pub(crate) fn run(args: &Args, repo_args: &RepoArgs) -> Result<()> {
    let metadata_out_dir = repo_args
        .outdir
        .join(&repo_args.variant)
        .join(&repo_args.arch);
    let targets_out_dir = repo_args.outdir.join("targets");

    // If the given metadata directory exists, throw an error.  We don't want to overwrite a user's
    // existing repository.  (The targets directory is shared, so it's fine if that exists.)
    ensure!(
        !Path::exists(&metadata_out_dir),
        error::RepoExists {
            path: metadata_out_dir
        }
    );

    // Build repo   =^..^=   =^..^=   =^..^=   =^..^=

    // If a lock file exists, use that, otherwise use Infra.toml or default
    let infra_config =
        InfraConfig::from_path_or_lock(&args.infra_config_path, true).context(error::Config)?;
    trace!("Using infra config: {:?}", infra_config);

    // If the user has the requested (or "default") repo defined in their Infra.toml, use it,
    // otherwise use a default config.
    let default_repo_config = RepoConfig::default();
    let repo_config = if let Some(repo_config) = infra_config
        .repo
        .as_ref()
        .and_then(|repo_section| repo_section.get(&repo_args.repo))
        .map(|repo| {
            info!("Using repo '{}' from Infra.toml", repo_args.repo);
            repo
        }) {
        repo_config
    } else {
        info!(
            "Didn't find repo '{}' in Infra.toml, using default configuration",
            repo_args.repo
        );
        &default_repo_config
    };

    // Build a repo editor and manifest, from an existing repo if available, otherwise fresh
    let maybe_urls = repo_urls(&repo_config, &repo_args.variant, &repo_args.arch)?;
    let (mut editor, mut manifest) = if let Some((metadata_url, targets_url)) = maybe_urls.as_ref()
    {
        info!("Found metadata and target URLs, loading existing repository");
        match load_editor_and_manifest(&repo_args.root_role_path, &metadata_url, &targets_url)? {
            Some((editor, manifest)) => (editor, manifest),
            None => {
                warn!(
                    "Did not find repo at '{}', starting a new one",
                    metadata_url
                );
                (
                    RepositoryEditor::new(&repo_args.root_role_path).context(error::NewEditor)?,
                    Manifest::default(),
                )
            }
        }
    } else {
        info!("Did not find metadata and target URLs in infra config, creating a new repository");
        (
            RepositoryEditor::new(&repo_args.root_role_path).context(error::NewEditor)?,
            Manifest::default(),
        )
    };

    // Add update information to manifest
    update_manifest(&repo_args, &mut manifest)?;
    // Write manifest to tempfile so it can be copied in as target later
    let manifest_path = NamedTempFile::new()
        .context(error::TempFile)?
        .into_temp_path();
    update_metadata::write_file(&manifest_path, &manifest).context(error::ManifestWrite {
        path: &manifest_path,
    })?;

    // Add manifest and targets to editor
    let copy_targets = &repo_args.copy_targets;
    let link_targets = repo_args.link_targets.iter().chain(vec![
        &repo_args.boot_image,
        &repo_args.root_image,
        &repo_args.hash_image,
    ]);
    let all_targets = copy_targets.iter().chain(link_targets.clone());

    update_editor(&repo_args, &mut editor, all_targets, &manifest_path)?;

    // Sign repo   =^..^=   =^..^=   =^..^=   =^..^=

    // Check if we have a signing key defined in Infra.toml; if not, we'll fall back to the
    // generated local key.
    let signing_key_config = repo_config.signing_keys.as_ref();

    let key_source = if let Some(signing_key_config) = signing_key_config {
        get_signing_key_source(signing_key_config)?
    } else {
        ensure!(
            repo_args.default_key_path.exists(),
            error::MissingConfig {
                missing: "signing_keys in repo config, and we found no local key",
            }
        );
        Box::new(LocalKeySource {
            path: repo_args.default_key_path.clone(),
        })
    };

    let signed_repo = editor.sign(&[key_source]).context(error::RepoSign)?;

    // Write repo   =^..^=   =^..^=   =^..^=   =^..^=

    // Write targets first so we don't have invalid metadata if targets fail
    info!("Writing repo targets to: {}", targets_out_dir.display());
    fs::create_dir_all(&targets_out_dir).context(error::CreateDir {
        path: &targets_out_dir,
    })?;

    // Copy manifest with proper name instead of tempfile name
    debug!("Copying manifest.json into {}", targets_out_dir.display());
    let target = "manifest.json";
    let target = target
        .try_into()
        .context(error::ParseTargetName { target })?;
    signed_repo
        .copy_target(
            &manifest_path,
            &targets_out_dir,
            // We should never have matching manifests from different repos
            PathExists::Fail,
            Some(&target),
        )
        .context(error::CopyTarget {
            target: &manifest_path,
            path: &targets_out_dir,
        })?;

    // Copy / link any other user requested targets
    for copy_target in copy_targets {
        debug!(
            "Copying target '{}' into {}",
            copy_target.display(),
            targets_out_dir.display()
        );
        signed_repo
            .copy_target(copy_target, &targets_out_dir, PathExists::Skip, None)
            .context(error::CopyTarget {
                target: copy_target,
                path: &targets_out_dir,
            })?;
    }
    for link_target in link_targets {
        debug!(
            "Linking target '{}' into {}",
            link_target.display(),
            targets_out_dir.display()
        );
        signed_repo
            .link_target(link_target, &targets_out_dir, PathExists::Skip, None)
            .context(error::LinkTarget {
                target: link_target,
                path: &targets_out_dir,
            })?;
    }

    info!("Writing repo metadata to: {}", metadata_out_dir.display());
    fs::create_dir_all(&metadata_out_dir).context(error::CreateDir {
        path: &metadata_out_dir,
    })?;
    signed_repo
        .write(&metadata_out_dir)
        .context(error::RepoWrite {
            path: &repo_args.outdir,
        })?;

    Ok(())
}

mod error {
    use chrono::{DateTime, Utc};
    use snafu::Snafu;
    use std::io;
    use std::path::PathBuf;
    use url::Url;

    #[derive(Debug, Snafu)]
    #[snafu(visibility = "pub(super)")]
    pub(crate) enum Error {
        #[snafu(display("Failed to add new update to manifest: {}", source))]
        AddUpdate {
            source: update_metadata::error::Error,
        },

        #[snafu(display("Failed to add new target '{}' to repo: {}", path.display(), source))]
        AddTarget {
            path: PathBuf,
            source: tough::error::Error,
        },

        #[snafu(display("Failed to build target metadata from path '{}': {}", path.display(), source))]
        BuildTarget {
            path: PathBuf,
            source: tough::schema::Error,
        },

        #[snafu(display("Failed to copy target '{}' to '{}': {}", target.display(), path.display(), source))]
        CopyTarget {
            target: PathBuf,
            path: PathBuf,
            source: tough::error::Error,
        },

        #[snafu(display("Error reading config: {}", source))]
        Config { source: pubsys_config::Error },

        #[snafu(display("Failed to create directory '{}': {}", path.display(), source))]
        CreateDir { path: PathBuf, source: io::Error },

        #[snafu(display("Failed to create repo editor from given repo: {}", source))]
        EditorFromRepo { source: tough::error::Error },

        #[snafu(display("Failed to read '{}': {}", path.display(), source))]
        File { path: PathBuf, source: io::Error },

        #[snafu(display("Invalid path given for image file: '{}'", path.display()))]
        InvalidImagePath { path: PathBuf },

        #[snafu(display("Invalid config file at '{}': {}", path.display(), source))]
        InvalidJson {
            path: PathBuf,
            source: serde_json::Error,
        },

        #[snafu(display("Failed to symlink target '{}' to '{}': {}", target.display(), path.display(), source))]
        LinkTarget {
            target: PathBuf,
            path: PathBuf,
            source: tough::error::Error,
        },

        #[snafu(display("Failed to write Manifest to '{}': {}", path.display(), source))]
        ManifestWrite {
            path: PathBuf,
            source: update_metadata::error::Error,
        },

        #[snafu(display("Infra.toml is missing {}", missing))]
        MissingConfig { missing: String },

        #[snafu(display("Repo URLs not specified for repo '{}'", repo))]
        MissingRepoUrls { repo: String },

        #[snafu(display("Failed to create new repo editor: {}", source))]
        NewEditor { source: tough::error::Error },

        #[snafu(display("Repo does not have a manifest.json: {}", metadata_url))]
        NoManifest { metadata_url: Url },

        #[snafu(display("Non-UTF8 path '{}' not supported", path.display()))]
        NonUtf8Path { path: PathBuf },

        #[snafu(display("Failed to parse {} to a valid rusoto region: {}", what, source))]
        ParseRegion {
            what: String,
            source: rusoto_core::region::ParseRegionError,
        },

        #[snafu(display("Invalid URL '{}': {}", input, source))]
        ParseUrl {
            input: String,
            source: url::ParseError,
        },

        #[snafu(display("Failed to read target '{}' from repo: {}", target, source))]
        ReadTarget {
            target: String,
            source: tough::error::Error,
        },

        #[snafu(display("Failed to parse target name from string '{}': {}", target, source))]
        ParseTargetName {
            target: String,
            source: tough::error::Error,
        },

        #[snafu(display("Repo exists at '{}' - remove it and try again", path.display()))]
        RepoExists { path: PathBuf },

        #[snafu(display("Could not fetch repo at '{}': {}", url, msg))]
        RepoFetch { url: Url, msg: String },

        #[snafu(display(
            "Failed to load repo from metadata URL '{}': {}",
            metadata_base_url,
            source
        ))]
        RepoLoad {
            metadata_base_url: Url,
            source: tough::error::Error,
        },

        #[snafu(display("Requested repository does not exist: '{}'", url))]
        RepoNotFound { url: Url },

        #[snafu(display("Failed to sign repository: {}", source))]
        RepoSign { source: tough::error::Error },

        #[snafu(display("Failed to write repository to {}: {}", path.display(), source))]
        RepoWrite {
            path: PathBuf,
            source: tough::error::Error,
        },

        #[snafu(display("Failed to set targets expiration to {}: {}", expiration, source))]
        SetTargetsExpiration {
            expiration: DateTime<Utc>,
            source: tough::error::Error,
        },

        #[snafu(display("Failed to set targets version to {}: {}", version, source))]
        SetTargetsVersion {
            version: u64,
            source: tough::error::Error,
        },

        #[snafu(display("Failed to set waves from '{}': {}", wave_policy_path.display(), source))]
        SetWaves {
            wave_policy_path: PathBuf,
            source: update_metadata::error::Error,
        },

        #[snafu(display("Failed to create temporary file: {}", source))]
        TempFile { source: io::Error },

        #[snafu(display("Failed to read update metadata '{}': {}", path.display(), source))]
        UpdateMetadataRead {
            path: PathBuf,
            source: update_metadata::error::Error,
        },
    }
}
pub(crate) use error::Error;
type Result<T> = std::result::Result<T, error::Error>;
