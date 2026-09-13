use serde_json::{Value, json};
use superlight_core::{actions::Platform, config, policy};
use superlight_ipc::{Request, Snapshot};

pub struct Editor {
    pub draft: Value,
    pub selected_profile: String,
    pub selected_button: usize,
    pub latest: Option<Snapshot>,
    baseline: Value,
    instance: Option<String>,
    revision: Option<u64>,
}

impl Default for Editor {
    fn default() -> Self {
        let value = config::defaults();
        Self {
            draft: value.clone(),
            baseline: value,
            selected_profile: "default".into(),
            selected_button: 0,
            latest: None,
            instance: None,
            revision: None,
        }
    }
}

impl Editor {
    pub fn dirty(&self) -> bool {
        self.draft != self.baseline
    }

    pub fn conflicted(&self) -> bool {
        self.latest.as_ref().is_some_and(|state| {
            self.instance.as_deref() != Some(state.instance.as_str())
                || self.revision != Some(state.revision)
        })
    }

    pub fn receive(&mut self, state: Snapshot) {
        if !self.dirty() {
            self.replace_baseline(&state);
        }
        self.latest = Some(state);
    }

    pub fn saved(&mut self, state: Snapshot) {
        self.replace_baseline(&state);
        self.latest = Some(state);
    }

    fn replace_baseline(&mut self, state: &Snapshot) {
        self.draft = state.config.clone();
        self.baseline = state.config.clone();
        self.instance = Some(state.instance.clone());
        self.revision = Some(state.revision);
        if self.draft["profiles"].get(&self.selected_profile).is_none() {
            self.selected_profile = "default".into();
        }
    }

    pub fn reload(&mut self) -> Result<(), String> {
        let state = self.latest.clone().ok_or("The service is not connected")?;
        self.replace_baseline(&state);
        Ok(())
    }

    pub fn apply_request(&self) -> Result<Request, String> {
        let revision = self.revision.ok_or("The service is not connected")?;
        if self.conflicted() {
            return Err("The service configuration changed. Reload before saving.".into());
        }
        let bytes = serde_json::to_vec(&self.draft).map_err(|error| error.to_string())?;
        let value = config::parse(&bytes)?;
        policy::validate_actions(&value, Platform::current())?;
        Ok(Request::Apply {
            expected_revision: revision,
            config: value,
        })
    }

    pub fn profiles(&self) -> Vec<(String, String)> {
        self.draft["profiles"]
            .as_object()
            .map(|profiles| {
                profiles
                    .iter()
                    .map(|(key, profile)| {
                        (
                            key.clone(),
                            profile["label"].as_str().unwrap_or(key).to_owned(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn add_profile(&mut self, label: &str) -> Result<String, String> {
        let label = label.trim();
        if label.is_empty() || label.chars().count() > 100 {
            return Err("Profile names must contain 1 to 100 characters".into());
        }
        let mappings = self.draft["profiles"]["default"]["mappings"].clone();
        let profiles = self.draft["profiles"]
            .as_object_mut()
            .ok_or("The profile configuration is invalid")?;
        if profiles.len() >= 64 {
            return Err("At most 64 profiles are supported".into());
        }
        let key = (1..=64)
            .map(|number| format!("profile_{number}"))
            .find(|key| !profiles.contains_key(key))
            .ok_or("No profile slot is available")?;
        profiles.insert(
            key.clone(),
            json!({"label": label, "apps": [], "mappings": mappings}),
        );
        self.selected_profile = key.clone();
        Ok(key)
    }

    pub fn remove_profile(&mut self, key: &str) -> Result<(), String> {
        if key == "default" {
            return Err("The default profile is required".into());
        }
        let profiles = self.draft["profiles"]
            .as_object_mut()
            .ok_or("The profile configuration is invalid")?;
        if profiles.remove(key).is_none() {
            return Err("The profile no longer exists".into());
        }
        if self.selected_profile == key {
            self.selected_profile = "default".into();
        }
        Ok(())
    }

    pub fn applications(&self) -> String {
        self.draft["profiles"][&self.selected_profile]["apps"]
            .as_array()
            .map(|apps| {
                apps.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    }

    pub fn set_applications(&mut self, text: &str) -> Result<(), String> {
        let mut apps = Vec::new();
        for name in text.lines().map(str::trim).filter(|name| !name.is_empty()) {
            if name.len() > 1024 {
                return Err("An application identifier is too long".into());
            }
            if apps
                .iter()
                .any(|app: &String| app.eq_ignore_ascii_case(name))
            {
                continue;
            }
            if apps.len() == 64 {
                return Err("At most 64 application identifiers are supported per profile".into());
            }
            apps.push(name.to_owned());
        }
        self.draft["profiles"][&self.selected_profile]["apps"] = json!(apps);
        Ok(())
    }

    pub fn set_action(&mut self, index: usize, action: &str) -> Result<(), String> {
        let button = config::BUTTONS.get(index).ok_or("Unknown mouse control")?;
        superlight_core::actions::Action::parse(action, Platform::current())?;
        self.draft["profiles"][&self.selected_profile]["mappings"][*button] = json!(action);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(instance: &str, revision: u64) -> Snapshot {
        Snapshot {
            instance: instance.into(),
            revision,
            config: config::defaults(),
            active_profile: "default".into(),
            ..Snapshot::default()
        }
    }

    #[test]
    fn disconnected_editor_cannot_build_a_save_request() {
        assert!(Editor::default().apply_request().is_err());
    }

    #[test]
    fn polling_preserves_unsaved_changes_until_the_service_revision_changes() {
        let mut editor = Editor::default();
        editor.receive(snapshot("service-a", 7));
        editor.draft["settings"]["dpi"] = json!(2400);
        editor.receive(snapshot("service-a", 7));
        assert!(editor.dirty());
        assert!(!editor.conflicted());
        assert_eq!(editor.draft["settings"]["dpi"], 2400);
        editor.receive(snapshot("service-a", 8));
        assert!(editor.conflicted());
        assert_eq!(editor.draft["settings"]["dpi"], 2400);
        assert!(editor.apply_request().is_err());
    }

    #[test]
    fn service_restart_invalidates_an_unsaved_revision() {
        let mut editor = Editor::default();
        editor.receive(snapshot("service-a", 4));
        editor.draft["settings"]["dpi"] = json!(1600);
        editor.receive(snapshot("service-b", 4));
        assert!(editor.conflicted());
        assert!(editor.apply_request().is_err());
    }

    #[test]
    fn explicit_reload_accepts_the_latest_revision() {
        let mut editor = Editor::default();
        editor.receive(snapshot("service-a", 4));
        editor.draft["settings"]["dpi"] = json!(1600);
        editor.receive(snapshot("service-a", 5));
        editor.reload().unwrap();
        assert!(!editor.dirty());
        assert!(!editor.conflicted());
        let Request::Apply {
            expected_revision, ..
        } = editor.apply_request().unwrap()
        else {
            panic!()
        };
        assert_eq!(expected_revision, 5);
    }

    #[test]
    fn unknown_fields_survive_editor_changes() {
        let mut state = snapshot("service-a", 1);
        state.config["future_extension"] = json!({"keep": [1, 2, 3]});
        let mut editor = Editor::default();
        editor.receive(state);
        editor.draft["settings"]["dpi"] = json!(1600);
        let Request::Apply { config, .. } = editor.apply_request().unwrap() else {
            panic!()
        };
        assert_eq!(config["future_extension"], json!({"keep": [1, 2, 3]}));
        assert_eq!(config["settings"]["dpi"], 1600);
    }

    #[test]
    fn default_profile_is_immutable_but_new_profiles_are_independent() {
        let mut editor = Editor::default();
        editor.receive(snapshot("service-a", 1));
        assert!(editor.remove_profile("default").is_err());
        let key = editor.add_profile("Browser").unwrap();
        editor.draft["profiles"][&key]["mappings"]["middle"] = json!("copy");
        assert_eq!(editor.draft["profiles"]["default"]["mappings"]["middle"], "none");
        assert_eq!(editor.draft["profiles"][&key]["label"], "Browser");
        editor.remove_profile(&key).unwrap();
        assert_eq!(editor.selected_profile, "default");
    }

    #[test]
    fn application_aliases_are_deduplicated_and_bounded() {
        let mut editor = Editor::default();
        editor.receive(snapshot("service-a", 1));
        let key = editor.add_profile("Browser").unwrap();
        editor.selected_profile = key;
        editor.set_applications("Firefox\nfirefox\norg.mozilla.firefox").unwrap();
        assert_eq!(editor.applications(), "Firefox\norg.mozilla.firefox");
        assert!(editor.set_applications(&"x\n".repeat(65)).is_err());
    }

    #[test]
    fn invalid_shortcuts_are_rejected_before_saving() {
        let mut editor = Editor::default();
        editor.receive(snapshot("service-a", 1));
        assert!(editor.set_action(0, "custom:not_a_real_key").is_err());
        assert!(editor.set_action(99, "copy").is_err());
        editor.draft["profiles"]["default"]["mappings"]["middle"] = json!("custom:not_a_real_key");
        assert!(editor.apply_request().is_err());
    }

    #[test]
    fn successful_save_replaces_the_editor_baseline() {
        let mut editor = Editor::default();
        editor.receive(snapshot("service-a", 1));
        editor.draft["settings"]["dpi"] = json!(1600);
        let mut saved = snapshot("service-a", 2);
        saved.config = editor.draft.clone();
        editor.saved(saved);
        assert!(!editor.dirty());
        let Request::Apply {
            expected_revision, ..
        } = editor.apply_request().unwrap()
        else {
            panic!()
        };
        assert_eq!(expected_revision, 2);
    }
}
