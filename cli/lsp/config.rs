// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

use super::logging::lsp_log;
use crate::args::ConfigFile;
use crate::lsp::logging::lsp_warn;
use crate::util::fs::canonicalize_path_maybe_not_exists;
use crate::util::path::specifier_to_file_path;
use deno_core::error::AnyError;
use deno_core::parking_lot::Mutex;
use deno_core::serde::Deserialize;
use deno_core::serde::Serialize;
use deno_core::serde_json;
use deno_core::serde_json::Value;
use deno_core::ModuleSpecifier;
use deno_lockfile::Lockfile;
use lsp::Url;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::lsp_types as lsp;

pub const SETTINGS_SECTION: &str = "deno";

#[derive(Debug, Clone, Default)]
pub struct ClientCapabilities {
  pub code_action_disabled_support: bool,
  pub line_folding_only: bool,
  pub snippet_support: bool,
  pub status_notification: bool,
  /// The client provides the `experimental.testingApi` capability, which is
  /// built around VSCode's testing API. It indicates that the server should
  /// send notifications about tests discovered in modules.
  pub testing_api: bool,
  pub workspace_configuration: bool,
  pub workspace_did_change_watched_files: bool,
  pub workspace_will_rename_files: bool,
}

fn is_true() -> bool {
  true
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodeLensSettings {
  /// Flag for providing implementation code lenses.
  #[serde(default)]
  pub implementations: bool,
  /// Flag for providing reference code lenses.
  #[serde(default)]
  pub references: bool,
  /// Flag for providing reference code lens on all functions.  For this to have
  /// an impact, the `references` flag needs to be `true`.
  #[serde(default)]
  pub references_all_functions: bool,
  /// Flag for providing test code lens on `Deno.test` statements.  There is
  /// also the `test_args` setting, but this is not used by the server.
  #[serde(default = "is_true")]
  pub test: bool,
}

impl Default for CodeLensSettings {
  fn default() -> Self {
    Self {
      implementations: false,
      references: false,
      references_all_functions: false,
      test: true,
    }
  }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodeLensSpecifierSettings {
  /// Flag for providing test code lens on `Deno.test` statements.  There is
  /// also the `test_args` setting, but this is not used by the server.
  #[serde(default = "is_true")]
  pub test: bool,
}

impl Default for CodeLensSpecifierSettings {
  fn default() -> Self {
    Self { test: true }
  }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompletionSettings {
  #[serde(default)]
  pub complete_function_calls: bool,
  #[serde(default = "is_true")]
  pub names: bool,
  #[serde(default = "is_true")]
  pub paths: bool,
  #[serde(default = "is_true")]
  pub auto_imports: bool,
  #[serde(default)]
  pub imports: ImportCompletionSettings,
}

impl Default for CompletionSettings {
  fn default() -> Self {
    Self {
      complete_function_calls: false,
      names: true,
      paths: true,
      auto_imports: true,
      imports: ImportCompletionSettings::default(),
    }
  }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InlayHintsSettings {
  #[serde(default)]
  pub parameter_names: InlayHintsParamNamesOptions,
  #[serde(default)]
  pub parameter_types: InlayHintsParamTypesOptions,
  #[serde(default)]
  pub variable_types: InlayHintsVarTypesOptions,
  #[serde(default)]
  pub property_declaration_types: InlayHintsPropDeclTypesOptions,
  #[serde(default)]
  pub function_like_return_types: InlayHintsFuncLikeReturnTypesOptions,
  #[serde(default)]
  pub enum_member_values: InlayHintsEnumMemberValuesOptions,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InlayHintsParamNamesOptions {
  #[serde(default)]
  pub enabled: InlayHintsParamNamesEnabled,
  #[serde(default = "is_true")]
  pub suppress_when_argument_matches_name: bool,
}

impl Default for InlayHintsParamNamesOptions {
  fn default() -> Self {
    Self {
      enabled: InlayHintsParamNamesEnabled::None,
      suppress_when_argument_matches_name: true,
    }
  }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InlayHintsParamNamesEnabled {
  None,
  Literals,
  All,
}

impl Default for InlayHintsParamNamesEnabled {
  fn default() -> Self {
    Self::None
  }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InlayHintsParamTypesOptions {
  #[serde(default)]
  pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InlayHintsVarTypesOptions {
  #[serde(default)]
  pub enabled: bool,
  #[serde(default = "is_true")]
  pub suppress_when_type_matches_name: bool,
}

impl Default for InlayHintsVarTypesOptions {
  fn default() -> Self {
    Self {
      enabled: false,
      suppress_when_type_matches_name: true,
    }
  }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InlayHintsPropDeclTypesOptions {
  #[serde(default)]
  pub enabled: bool,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InlayHintsFuncLikeReturnTypesOptions {
  #[serde(default)]
  pub enabled: bool,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InlayHintsEnumMemberValuesOptions {
  #[serde(default)]
  pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportCompletionSettings {
  /// A flag that indicates if non-explicitly set origins should be checked for
  /// supporting import suggestions.
  #[serde(default = "is_true")]
  pub auto_discover: bool,
  /// A map of origins which have had explicitly set if import suggestions are
  /// enabled.
  #[serde(default)]
  pub hosts: HashMap<String, bool>,
}

impl Default for ImportCompletionSettings {
  fn default() -> Self {
    Self {
      auto_discover: true,
      hosts: HashMap::default(),
    }
  }
}

/// Deno language server specific settings that can be applied uniquely to a
/// specifier.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SpecifierSettings {
  /// A flag that indicates if Deno is enabled for this specifier or not.
  pub enable: Option<bool>,
  /// A list of paths, using the workspace folder as a base that should be Deno
  /// enabled.
  #[serde(default)]
  pub enable_paths: Vec<String>,
  /// Code lens specific settings for the resource.
  #[serde(default)]
  pub code_lens: CodeLensSpecifierSettings,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TestingSettings {
  /// A vector of arguments which should be used when running the tests for
  /// a workspace.
  #[serde(default)]
  pub args: Vec<String>,
}

impl Default for TestingSettings {
  fn default() -> Self {
    Self {
      args: vec!["--allow-all".to_string(), "--no-check".to_string()],
    }
  }
}

fn default_to_true() -> bool {
  true
}

fn default_document_preload_limit() -> usize {
  1000
}

fn empty_string_none<'de, D: serde::Deserializer<'de>>(
  d: D,
) -> Result<Option<String>, D::Error> {
  let o: Option<String> = Option::deserialize(d)?;
  Ok(o.filter(|s| !s.is_empty()))
}

/// Deno language server specific settings that are applied to a workspace.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSettings {
  /// A flag that indicates if Deno is enabled for the workspace.
  pub enable: Option<bool>,

  /// A list of paths, using the root_uri as a base that should be Deno enabled.
  #[serde(default)]
  pub enable_paths: Vec<String>,

  /// An option that points to a path string of the path to utilise as the
  /// cache/DENO_DIR for the language server.
  #[serde(default, deserialize_with = "empty_string_none")]
  pub cache: Option<String>,

  /// Override the default stores used to validate certificates. This overrides
  /// the environment variable `DENO_TLS_CA_STORE` if present.
  pub certificate_stores: Option<Vec<String>>,

  /// An option that points to a path string of the config file to apply to
  /// code within the workspace.
  #[serde(default, deserialize_with = "empty_string_none")]
  pub config: Option<String>,

  /// An option that points to a path string of the import map to apply to the
  /// code within the workspace.
  #[serde(default, deserialize_with = "empty_string_none")]
  pub import_map: Option<String>,

  /// Code lens specific settings for the workspace.
  #[serde(default)]
  pub code_lens: CodeLensSettings,

  #[serde(default)]
  pub inlay_hints: InlayHintsSettings,

  /// A flag that indicates if internal debug logging should be made available.
  #[serde(default)]
  pub internal_debug: bool,

  /// A flag that indicates if linting is enabled for the workspace.
  #[serde(default = "default_to_true")]
  pub lint: bool,

  /// Limits the number of files that can be preloaded by the language server.
  #[serde(default = "default_document_preload_limit")]
  pub document_preload_limit: usize,

  /// A flag that indicates if Dene should validate code against the unstable
  /// APIs for the workspace.
  #[serde(default)]
  pub suggest: CompletionSettings,

  /// Testing settings for the workspace.
  #[serde(default)]
  pub testing: TestingSettings,

  /// An option which sets the cert file to use when attempting to fetch remote
  /// resources. This overrides `DENO_CERT` if present.
  #[serde(default, deserialize_with = "empty_string_none")]
  pub tls_certificate: Option<String>,

  /// An option, if set, will unsafely ignore certificate errors when fetching
  /// remote resources.
  #[serde(default)]
  pub unsafely_ignore_certificate_errors: Option<Vec<String>>,

  #[serde(default)]
  pub unstable: bool,
}

impl Default for WorkspaceSettings {
  fn default() -> Self {
    WorkspaceSettings {
      enable: None,
      enable_paths: vec![],
      cache: None,
      certificate_stores: None,
      config: None,
      import_map: None,
      code_lens: Default::default(),
      inlay_hints: Default::default(),
      internal_debug: false,
      lint: true,
      document_preload_limit: default_document_preload_limit(),
      suggest: Default::default(),
      testing: Default::default(),
      tls_certificate: None,
      unsafely_ignore_certificate_errors: None,
      unstable: false,
    }
  }
}

impl WorkspaceSettings {
  /// Determine if any code lenses are enabled at all.  This allows short
  /// circuiting when there are no code lenses enabled.
  pub fn enabled_code_lens(&self) -> bool {
    self.code_lens.implementations || self.code_lens.references
  }

  /// Determine if any inlay hints are enabled. This allows short circuiting
  /// when there are no inlay hints enabled.
  pub fn enabled_inlay_hints(&self) -> bool {
    !matches!(
      self.inlay_hints.parameter_names.enabled,
      InlayHintsParamNamesEnabled::None
    ) || self.inlay_hints.parameter_types.enabled
      || self.inlay_hints.variable_types.enabled
      || self.inlay_hints.property_declaration_types.enabled
      || self.inlay_hints.function_like_return_types.enabled
      || self.inlay_hints.enum_member_values.enabled
  }
}

#[derive(Debug, Clone, Default)]
pub struct ConfigSnapshot {
  pub client_capabilities: ClientCapabilities,
  pub enabled_paths: HashMap<Url, Vec<Url>>,
  pub excluded_paths: Option<Vec<Url>>,
  pub has_config_file: bool,
  pub settings: Settings,
}

impl ConfigSnapshot {
  /// Determine if the provided specifier is enabled or not.
  pub fn specifier_enabled(&self, specifier: &ModuleSpecifier) -> bool {
    specifier_enabled(
      &self.enabled_paths,
      self.excluded_paths.as_ref(),
      &self.settings,
      self.has_config_file,
      specifier,
    )
  }
}

#[derive(Debug, Default, Clone)]
pub struct Settings {
  pub specifiers: BTreeMap<ModuleSpecifier, SpecifierSettings>,
  pub workspace: WorkspaceSettings,
}

#[derive(Debug)]
struct WithCanonicalizedSpecifier<T> {
  /// Stored canonicalized specifier, which is used for file watcher events.
  canonicalized_specifier: ModuleSpecifier,
  file: T,
}

/// Contains the config file and dependent information.
#[derive(Debug)]
struct LspConfigFileInfo {
  config_file: WithCanonicalizedSpecifier<ConfigFile>,
  /// An optional deno.lock file, which is resolved relative to the config file.
  maybe_lockfile: Option<WithCanonicalizedSpecifier<Arc<Mutex<Lockfile>>>>,
  /// The canonicalized node_modules directory, which is found relative to the config file.
  maybe_node_modules_dir: Option<PathBuf>,
  excluded_paths: Vec<Url>,
}

#[derive(Debug)]
pub struct Config {
  pub client_capabilities: ClientCapabilities,
  enabled_paths: HashMap<Url, Vec<Url>>,
  pub root_uri: Option<ModuleSpecifier>,
  settings: Settings,
  pub workspace_folders: Option<Vec<(ModuleSpecifier, lsp::WorkspaceFolder)>>,
  /// An optional configuration file which has been specified in the client
  /// options along with some data that is computed after the config file is set.
  maybe_config_file_info: Option<LspConfigFileInfo>,
}

impl Config {
  pub fn new() -> Self {
    Self {
      client_capabilities: ClientCapabilities::default(),
      enabled_paths: Default::default(),
      /// Root provided by the initialization parameters.
      root_uri: None,
      settings: Default::default(),
      workspace_folders: None,
      maybe_config_file_info: None,
    }
  }

  pub fn maybe_node_modules_dir_path(&self) -> Option<&PathBuf> {
    self
      .maybe_config_file_info
      .as_ref()
      .and_then(|p| p.maybe_node_modules_dir.as_ref())
  }

  pub fn maybe_vendor_dir_path(&self) -> Option<PathBuf> {
    self.maybe_config_file().and_then(|c| c.vendor_dir_path())
  }

  pub fn maybe_config_file(&self) -> Option<&ConfigFile> {
    self
      .maybe_config_file_info
      .as_ref()
      .map(|c| &c.config_file.file)
  }

  /// Canonicalized specifier of the config file, which should only be used for
  /// file watcher events. Otherwise, prefer using the non-canonicalized path
  /// as the rest of the CLI does for config files.
  pub fn maybe_config_file_canonicalized_specifier(
    &self,
  ) -> Option<&ModuleSpecifier> {
    self
      .maybe_config_file_info
      .as_ref()
      .map(|c| &c.config_file.canonicalized_specifier)
  }

  pub fn maybe_lockfile(&self) -> Option<&Arc<Mutex<Lockfile>>> {
    self
      .maybe_config_file_info
      .as_ref()
      .and_then(|c| c.maybe_lockfile.as_ref().map(|l| &l.file))
  }

  /// Canonicalized specifier of the lockfile, which should only be used for
  /// file watcher events. Otherwise, prefer using the non-canonicalized path
  /// as the rest of the CLI does for config files.
  pub fn maybe_lockfile_canonicalized_specifier(
    &self,
  ) -> Option<&ModuleSpecifier> {
    self.maybe_config_file_info.as_ref().and_then(|c| {
      c.maybe_lockfile
        .as_ref()
        .map(|l| &l.canonicalized_specifier)
    })
  }

  pub fn clear_config_file(&mut self) {
    self.maybe_config_file_info = None;
  }

  pub fn has_config_file(&self) -> bool {
    self.maybe_config_file_info.is_some()
  }

  pub fn set_config_file(&mut self, config_file: ConfigFile) {
    self.maybe_config_file_info = Some(LspConfigFileInfo {
      maybe_lockfile: resolve_lockfile_from_config(&config_file).map(
        |lockfile| {
          let path = canonicalize_path_maybe_not_exists(&lockfile.filename)
            .unwrap_or_else(|_| lockfile.filename.clone());
          WithCanonicalizedSpecifier {
            canonicalized_specifier: ModuleSpecifier::from_file_path(path)
              .unwrap(),
            file: Arc::new(Mutex::new(lockfile)),
          }
        },
      ),
      maybe_node_modules_dir: resolve_node_modules_dir(&config_file),
      excluded_paths: config_file
        .to_files_config()
        .ok()
        .flatten()
        .map(|c| c.exclude)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|path| ModuleSpecifier::from_file_path(path).ok())
        .collect(),
      config_file: WithCanonicalizedSpecifier {
        canonicalized_specifier: config_file
          .specifier
          .to_file_path()
          .ok()
          .and_then(|p| canonicalize_path_maybe_not_exists(&p).ok())
          .and_then(|p| ModuleSpecifier::from_file_path(p).ok())
          .unwrap_or_else(|| config_file.specifier.clone()),
        file: config_file,
      },
    });
  }

  pub fn workspace_settings(&self) -> &WorkspaceSettings {
    &self.settings.workspace
  }

  /// Set the workspace settings directly, which occurs during initialization
  /// and when the client does not support workspace configuration requests
  pub fn set_workspace_settings(
    &mut self,
    value: Value,
  ) -> Result<(), AnyError> {
    let workspace_settings = serde_json::from_value(value)?;
    self.settings.workspace = workspace_settings;
    Ok(())
  }

  pub fn snapshot(&self) -> Arc<ConfigSnapshot> {
    Arc::new(ConfigSnapshot {
      client_capabilities: self.client_capabilities.clone(),
      enabled_paths: self.enabled_paths.clone(),
      excluded_paths: self
        .maybe_config_file_info
        .as_ref()
        .map(|i| i.excluded_paths.clone()),
      has_config_file: self.has_config_file(),
      settings: self.settings.clone(),
    })
  }

  pub fn has_specifier_settings(&self, specifier: &ModuleSpecifier) -> bool {
    self.settings.specifiers.contains_key(specifier)
  }

  pub fn enabled(&self) -> bool {
    self
      .settings
      .workspace
      .enable
      .unwrap_or_else(|| self.has_config_file())
  }

  pub fn specifier_enabled(&self, specifier: &ModuleSpecifier) -> bool {
    specifier_enabled(
      &self.enabled_paths,
      self
        .maybe_config_file_info
        .as_ref()
        .map(|i| &i.excluded_paths),
      &self.settings,
      self.has_config_file(),
      specifier,
    )
  }

  /// Gets the directories or specifically enabled file paths based on the
  /// workspace config.
  ///
  /// WARNING: This may incorrectly have some directory urls as being
  /// represented as file urls.
  pub fn enabled_urls(&self) -> Vec<Url> {
    let mut urls: Vec<Url> = Vec::new();

    if !self.enabled() && self.enabled_paths.is_empty() {
      // do not return any urls when disabled
      return urls;
    }

    for (workspace, enabled_paths) in &self.enabled_paths {
      if !enabled_paths.is_empty() {
        urls.extend(enabled_paths.iter().cloned());
      } else {
        urls.push(workspace.clone());
      }
    }

    if urls.is_empty() {
      if let Some(root_dir) = &self.root_uri {
        urls.push(root_dir.clone())
      }
    }

    // sort for determinism
    urls.sort();
    urls
  }

  pub fn specifier_code_lens_test(&self, specifier: &ModuleSpecifier) -> bool {
    let value = self
      .settings
      .specifiers
      .get(specifier)
      .map(|settings| settings.code_lens.test)
      .unwrap_or_else(|| self.settings.workspace.code_lens.test);
    value
  }

  pub fn update_capabilities(
    &mut self,
    capabilities: &lsp::ClientCapabilities,
  ) {
    if let Some(experimental) = &capabilities.experimental {
      self.client_capabilities.status_notification = experimental
        .get("statusNotification")
        .and_then(|it| it.as_bool())
        == Some(true);
      self.client_capabilities.testing_api =
        experimental.get("testingApi").and_then(|it| it.as_bool())
          == Some(true);
    }

    if let Some(workspace) = &capabilities.workspace {
      self.client_capabilities.workspace_configuration =
        workspace.configuration.unwrap_or(false);
      self.client_capabilities.workspace_did_change_watched_files = workspace
        .did_change_watched_files
        .and_then(|it| it.dynamic_registration)
        .unwrap_or(false);
      if let Some(file_operations) = &workspace.file_operations {
        if let Some(true) = file_operations.dynamic_registration {
          self.client_capabilities.workspace_will_rename_files =
            file_operations.will_rename.unwrap_or(false);
        }
      }
    }

    if let Some(text_document) = &capabilities.text_document {
      self.client_capabilities.line_folding_only = text_document
        .folding_range
        .as_ref()
        .and_then(|it| it.line_folding_only)
        .unwrap_or(false);
      self.client_capabilities.code_action_disabled_support = text_document
        .code_action
        .as_ref()
        .and_then(|it| it.disabled_support)
        .unwrap_or(false);
      self.client_capabilities.snippet_support =
        if let Some(completion) = &text_document.completion {
          completion
            .completion_item
            .as_ref()
            .and_then(|it| it.snippet_support)
            .unwrap_or(false)
        } else {
          false
        };
    }
  }

  /// Given the configured workspaces or root URI and the their settings,
  /// update and resolve any paths that should be enabled
  pub fn update_enabled_paths(&mut self) -> bool {
    if let Some(workspace_folders) = self.workspace_folders.clone() {
      let mut touched = false;
      for (workspace, _) in workspace_folders {
        let enabled_paths = match self.settings.specifiers.get(&workspace) {
          Some(settings) => settings.enable_paths.clone(),
          None => self.settings.workspace.enable_paths.clone(),
        };
        if self.update_enabled_paths_entry(workspace, enabled_paths) {
          touched = true;
        }
      }
      touched
    } else if let Some(root_uri) = self.root_uri.clone() {
      self.update_enabled_paths_entry(
        root_uri,
        self.settings.workspace.enable_paths.clone(),
      )
    } else {
      false
    }
  }

  /// Update a specific entry in the enabled paths for a given workspace.
  fn update_enabled_paths_entry(
    &mut self,
    workspace: ModuleSpecifier,
    enabled_paths: Vec<String>,
  ) -> bool {
    let mut touched = false;
    if !enabled_paths.is_empty() {
      if let Ok(workspace_path) = specifier_to_file_path(&workspace) {
        let mut paths = Vec::new();
        for path in &enabled_paths {
          let fs_path = workspace_path.join(path);
          match ModuleSpecifier::from_file_path(fs_path) {
            Ok(path_uri) => {
              paths.push(path_uri);
            }
            Err(_) => {
              lsp_log!("Unable to resolve a file path for `deno.enablePath` from \"{}\" for workspace \"{}\".", path, workspace);
            }
          }
        }
        if !paths.is_empty() {
          touched = true;
          self.enabled_paths.insert(workspace.clone(), paths);
        }
      }
    } else {
      touched = true;
      self.enabled_paths.remove(&workspace);
    }
    touched
  }

  pub fn get_specifiers(&self) -> Vec<ModuleSpecifier> {
    self.settings.specifiers.keys().cloned().collect()
  }

  pub fn set_specifier_settings(
    &mut self,
    specifier: ModuleSpecifier,
    settings: SpecifierSettings,
  ) -> bool {
    if let Some(existing) = self.settings.specifiers.get(&specifier) {
      if *existing == settings {
        return false;
      }
    }

    self.settings.specifiers.insert(specifier, settings);
    true
  }
}

fn specifier_enabled(
  enabled_paths: &HashMap<Url, Vec<Url>>,
  excluded_paths: Option<&Vec<Url>>,
  settings: &Settings,
  workspace_has_config_file: bool,
  specifier: &Url,
) -> bool {
  let specifier_str = specifier.as_str();
  for (workspace, enabled_paths) in enabled_paths.iter() {
    if specifier_str.starts_with(workspace.as_str()) {
      return enabled_paths
        .iter()
        .any(|path| specifier_str.starts_with(path.as_str()));
    }
  }
  if let Some(excluded_paths) = excluded_paths {
    for excluded_path in excluded_paths {
      if specifier_str.starts_with(excluded_path.as_str()) {
        return false;
      }
    }
  }
  settings
    .specifiers
    .get(specifier)
    .and_then(|settings| settings.enable)
    .or(settings.workspace.enable)
    .unwrap_or(workspace_has_config_file)
}

fn resolve_lockfile_from_config(config_file: &ConfigFile) -> Option<Lockfile> {
  let lockfile_path = match config_file.resolve_lockfile_path() {
    Ok(Some(value)) => value,
    Ok(None) => return None,
    Err(err) => {
      lsp_warn!("Error resolving lockfile: {:#}", err);
      return None;
    }
  };
  resolve_lockfile_from_path(lockfile_path)
}

fn resolve_node_modules_dir(config_file: &ConfigFile) -> Option<PathBuf> {
  // For the language server, require an explicit opt-in via the
  // `nodeModulesDir: true` setting in the deno.json file. This is to
  // reduce the chance of modifying someone's node_modules directory
  // without them having asked us to do so.
  let explicitly_disabled = config_file.node_modules_dir_flag() == Some(false);
  if explicitly_disabled {
    return None;
  }
  let enabled = config_file.node_modules_dir_flag() == Some(true)
    || config_file.vendor_dir_flag() == Some(true);
  if !enabled {
    return None;
  }
  if config_file.specifier.scheme() != "file" {
    return None;
  }
  let file_path = config_file.specifier.to_file_path().ok()?;
  let node_modules_dir = file_path.parent()?.join("node_modules");
  canonicalize_path_maybe_not_exists(&node_modules_dir).ok()
}

fn resolve_lockfile_from_path(lockfile_path: PathBuf) -> Option<Lockfile> {
  match Lockfile::new(lockfile_path, false) {
    Ok(value) => {
      if let Ok(specifier) = ModuleSpecifier::from_file_path(&value.filename) {
        lsp_log!("  Resolved lock file: \"{}\"", specifier);
      }
      Some(value)
    }
    Err(err) => {
      lsp_warn!("Error loading lockfile: {:#}", err);
      None
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use deno_core::resolve_url;
  use deno_core::serde_json::json;
  use pretty_assertions::assert_eq;

  #[test]
  fn test_config_specifier_enabled() {
    let mut config = Config::new();
    let specifier = resolve_url("file:///a.ts").unwrap();
    assert!(!config.specifier_enabled(&specifier));
    config
      .set_workspace_settings(json!({
        "enable": true
      }))
      .expect("could not update");
    assert!(config.specifier_enabled(&specifier));
  }

  #[test]
  fn test_config_snapshot_specifier_enabled() {
    let mut config = Config::new();
    let specifier = resolve_url("file:///a.ts").unwrap();
    assert!(!config.specifier_enabled(&specifier));
    config
      .set_workspace_settings(json!({
        "enable": true
      }))
      .expect("could not update");
    let config_snapshot = config.snapshot();
    assert!(config_snapshot.specifier_enabled(&specifier));
  }

  #[test]
  fn test_config_specifier_enabled_path() {
    let mut config = Config::new();
    let specifier_a = resolve_url("file:///project/worker/a.ts").unwrap();
    let specifier_b = resolve_url("file:///project/other/b.ts").unwrap();
    assert!(!config.specifier_enabled(&specifier_a));
    assert!(!config.specifier_enabled(&specifier_b));
    let mut enabled_paths = HashMap::new();
    enabled_paths.insert(
      Url::parse("file:///project/").unwrap(),
      vec![Url::parse("file:///project/worker/").unwrap()],
    );
    config.enabled_paths = enabled_paths;
    assert!(config.specifier_enabled(&specifier_a));
    assert!(!config.specifier_enabled(&specifier_b));
    let config_snapshot = config.snapshot();
    assert!(config_snapshot.specifier_enabled(&specifier_a));
    assert!(!config_snapshot.specifier_enabled(&specifier_b));
  }

  #[test]
  fn test_set_workspace_settings_defaults() {
    let mut config = Config::new();
    config
      .set_workspace_settings(json!({}))
      .expect("could not update");
    assert_eq!(
      config.workspace_settings().clone(),
      WorkspaceSettings {
        enable: None,
        enable_paths: Vec::new(),
        cache: None,
        certificate_stores: None,
        config: None,
        import_map: None,
        code_lens: CodeLensSettings {
          implementations: false,
          references: false,
          references_all_functions: false,
          test: true,
        },
        inlay_hints: InlayHintsSettings {
          parameter_names: InlayHintsParamNamesOptions {
            enabled: InlayHintsParamNamesEnabled::None,
            suppress_when_argument_matches_name: true
          },
          parameter_types: InlayHintsParamTypesOptions { enabled: false },
          variable_types: InlayHintsVarTypesOptions {
            enabled: false,
            suppress_when_type_matches_name: true
          },
          property_declaration_types: InlayHintsPropDeclTypesOptions {
            enabled: false
          },
          function_like_return_types: InlayHintsFuncLikeReturnTypesOptions {
            enabled: false
          },
          enum_member_values: InlayHintsEnumMemberValuesOptions {
            enabled: false
          },
        },
        internal_debug: false,
        lint: true,
        document_preload_limit: 1_000,
        suggest: CompletionSettings {
          complete_function_calls: false,
          names: true,
          paths: true,
          auto_imports: true,
          imports: ImportCompletionSettings {
            auto_discover: true,
            hosts: HashMap::new(),
          }
        },
        testing: TestingSettings {
          args: vec!["--allow-all".to_string(), "--no-check".to_string()],
        },
        tls_certificate: None,
        unsafely_ignore_certificate_errors: None,
        unstable: false,
      }
    );
  }

  #[test]
  fn test_empty_cache() {
    let mut config = Config::new();
    config
      .set_workspace_settings(json!({ "cache": "" }))
      .expect("could not update");
    assert_eq!(
      config.workspace_settings().clone(),
      WorkspaceSettings::default()
    );
  }

  #[test]
  fn test_empty_import_map() {
    let mut config = Config::new();
    config
      .set_workspace_settings(json!({ "import_map": "" }))
      .expect("could not update");
    assert_eq!(
      config.workspace_settings().clone(),
      WorkspaceSettings::default()
    );
  }

  #[test]
  fn test_empty_tls_certificate() {
    let mut config = Config::new();
    config
      .set_workspace_settings(json!({ "tls_certificate": "" }))
      .expect("could not update");
    assert_eq!(
      config.workspace_settings().clone(),
      WorkspaceSettings::default()
    );
  }

  #[test]
  fn test_empty_config() {
    let mut config = Config::new();
    config
      .set_workspace_settings(json!({ "config": "" }))
      .expect("could not update");
    assert_eq!(
      config.workspace_settings().clone(),
      WorkspaceSettings::default()
    );
  }

  #[test]
  fn config_enabled_urls() {
    let mut config = Config::new();
    let root_dir = Url::parse("file:///example/").unwrap();
    config.root_uri = Some(root_dir.clone());
    config.settings.workspace.enable = Some(false);
    config.settings.workspace.enable_paths = Vec::new();
    assert_eq!(config.enabled_urls(), vec![]);

    config.settings.workspace.enable = Some(true);
    assert_eq!(config.enabled_urls(), vec![root_dir]);

    config.settings.workspace.enable = Some(false);
    let root_dir1 = Url::parse("file:///root1/").unwrap();
    let root_dir2 = Url::parse("file:///root2/").unwrap();
    let root_dir3 = Url::parse("file:///root3/").unwrap();
    config.enabled_paths = HashMap::from([
      (
        root_dir1.clone(),
        vec![
          root_dir1.join("sub_dir/").unwrap(),
          root_dir1.join("sub_dir/other/").unwrap(),
          root_dir1.join("test.ts").unwrap(),
        ],
      ),
      (root_dir2.clone(), vec![root_dir2.join("other.ts").unwrap()]),
      (root_dir3.clone(), vec![]),
    ]);

    assert_eq!(
      config.enabled_urls(),
      vec![
        root_dir1.join("sub_dir/").unwrap(),
        root_dir1.join("sub_dir/other/").unwrap(),
        root_dir1.join("test.ts").unwrap(),
        root_dir2.join("other.ts").unwrap(),
        root_dir3
      ]
    );
  }

  #[test]
  fn config_enable_via_config_file_detection() {
    let mut config = Config::new();
    let root_uri = Url::parse("file:///root/").unwrap();
    config.root_uri = Some(root_uri.clone());
    config.settings.workspace.enable = None;
    assert_eq!(config.enabled_urls(), vec![]);

    config.set_config_file(
      ConfigFile::new("{}", root_uri.join("deno.json").unwrap()).unwrap(),
    );
    assert_eq!(config.enabled_urls(), vec![root_uri]);
  }
}
