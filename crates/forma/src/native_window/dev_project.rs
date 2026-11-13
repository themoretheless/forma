//! The scene the dev window is started with, loaded from disk and reloaded when it changes.

use std::time::{Duration, Instant};

type FileStamp = (std::time::SystemTime, u64);

fn file_stamp(path: Option<&str>) -> Option<FileStamp> {
    let meta = std::fs::metadata(path?).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// The files the dev window was started from, watched for changes.
pub struct HotReload {
    pub(super) source_path: Option<String>,
    pub(super) template_path: Option<String>,
    pub(super) source_meta: Option<FileStamp>,
    pub(super) template_meta: Option<FileStamp>,
    pub(super) project_root: Option<std::path::PathBuf>,
    pub(super) project_entry: Option<String>,
    pub(super) last_project_files: std::collections::BTreeMap<String, String>,
    pub(super) last_source: String,
    pub(super) last_template: String,
    pub(super) next_hot_check: Instant,
}

impl HotReload {
    /// The built-in two-button demo: nothing on disk to watch.
    pub fn is_demo(&self) -> bool {
        self.source_path.is_none() && self.project_root.is_none()
    }

    /// When the files are looked at next, if anything is watched.
    pub fn deadline(&self) -> Option<Instant> {
        (!self.is_demo()).then_some(self.next_hot_check)
    }

    /// Looks at the watched files when the check is due. Returns the runtime rebuilt from changed
    /// files and the line to print; a change that does not compile keeps the current runtime.
    pub fn poll(
        &mut self,
        now: Instant,
        current: &crate::Runtime,
    ) -> Option<(crate::Runtime, &'static str)> {
        if now < self.next_hot_check {
            return None;
        }
        if let (Some(root), Some(entry)) = (&self.project_root, &self.project_entry) {
            self.next_hot_check = now + Duration::from_millis(150);
            let next_files = collect_ui_project_files(root).ok()?;
            if next_files == self.last_project_files {
                return None;
            }
            let prev_state = current.ui_state_properties();
            let next = crate::Runtime::from_ui_files(&next_files, entry, &prev_state).ok()?;
            self.last_project_files = next_files;
            return Some((
                next,
                "Hot reload: многофайловый .ui проект перекомпилирован в Rust",
            ));
        }
        self.source_path.as_ref()?;
        self.next_hot_check = now + Duration::from_millis(120);
        let cur_src_meta = file_stamp(self.source_path.as_deref());
        let cur_tpl_meta = file_stamp(self.template_path.as_deref());
        let src_meta_changed = cur_src_meta.is_none() || cur_src_meta != self.source_meta;
        let tpl_meta_changed = self.template_path.is_some()
            && (cur_tpl_meta.is_none() || cur_tpl_meta != self.template_meta);
        if !src_meta_changed && !tpl_meta_changed {
            return None;
        }
        let read = |changed: bool, path: &Option<String>| {
            changed
                .then(|| std::fs::read_to_string(path.as_deref()?).ok())
                .flatten()
        };
        let next_src = read(src_meta_changed, &self.source_path);
        let next_tpl = read(tpl_meta_changed, &self.template_path);
        if next_src.is_some() {
            self.source_meta = cur_src_meta;
        }
        if next_tpl.is_some() {
            self.template_meta = cur_tpl_meta;
        }
        let src_changed = next_src
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty() && s != &self.last_source);
        let tpl_changed = next_tpl
            .as_ref()
            .is_some_and(|t| !t.trim().is_empty() && t != &self.last_template);
        if !src_changed && !tpl_changed {
            return None;
        }
        let source = next_src.unwrap_or_else(|| self.last_source.clone());
        let template = next_tpl.unwrap_or_else(|| self.last_template.clone());
        let next = crate::Runtime::from_sources(&source, &template).ok()?;
        self.last_source = source;
        self.last_template = template;
        Some((next, "Hot reload: векторная сцена обновлена на лету"))
    }
}

pub fn collect_ui_project_files(
    root: &std::path::Path,
) -> std::io::Result<std::collections::BTreeMap<String, String>> {
    fn walk(
        base: &std::path::Path,
        dir: &std::path::Path,
        out: &mut std::collections::BTreeMap<String, String>,
    ) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.')
                || name_str == "target"
                || name_str == "node_modules"
                || name_str == "dist"
            {
                continue;
            }
            if path.is_dir() {
                walk(base, &path, out)?;
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("ui" | "svg")
            ) {
                if let Ok(rel) = path.strip_prefix(base) {
                    let key = rel
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/");
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        out.insert(key, content);
                    }
                }
            }
        }
        Ok(())
    }
    let mut files = std::collections::BTreeMap::new();
    walk(root, root, &mut files)?;
    Ok(files)
}

pub fn resolve_ui_project_root_and_entry(
    input_path: &std::path::Path,
) -> Option<(std::path::PathBuf, String)> {
    if input_path.is_dir() {
        let root = input_path.canonicalize().ok()?;
        let files = collect_ui_project_files(&root).ok()?;
        let entry = files
            .keys()
            .find(|k| {
                k.ends_with(".ui")
                    && !k.ends_with(".design.ui")
                    && !k.ends_with(".component.ui")
                    && !k.starts_with("components/")
            })
            .or_else(|| {
                files
                    .keys()
                    .find(|k| k.ends_with(".ui") && !k.ends_with(".design.ui"))
            })?
            .clone();
        return Some((root, entry));
    }
    let canon = input_path.canonicalize().ok()?;
    let parent = canon.parent()?;
    let root = if matches!(
        parent.file_name().and_then(|n| n.to_str()),
        Some("ui" | "components" | "src" | "pages")
    ) {
        parent.parent().unwrap_or(parent).to_path_buf()
    } else {
        parent.to_path_buf()
    };
    let rel = canon.strip_prefix(&root).ok()?;
    let entry = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    Some((root, entry))
}

/// The scene the dev window starts with and the files to watch for it.
pub struct DevProject {
    pub runtime: crate::Runtime,
    pub source: String,
    pub template: String,
    pub hot: HotReload,
}

impl DevProject {
    /// One argument is a `.ui` file or a project folder, two are a scene and its component
    /// template, none is the built-in two-button demo.
    pub fn load(
        source_path: Option<String>,
        template_path: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut project_root = None;
        let mut project_entry = None;
        let mut last_project_files = std::collections::BTreeMap::new();
        let mut source = String::new();
        let mut template = String::new();

        let runtime = if let (Some(first_arg), None) = (&source_path, &template_path) {
            let p = std::path::Path::new(first_arg);
            if p.is_dir() {
                let (root, entry) = resolve_ui_project_root_and_entry(p)
                    .ok_or("Не найден входной .ui файл в указанной папке")?;
                let files = collect_ui_project_files(&root)?;
                let rt = crate::Runtime::from_ui_files(
                    &files,
                    &entry,
                    &crate::form_document::Properties::new(),
                )
                .map_err(std::io::Error::other)?;
                project_root = Some(root);
                project_entry = Some(entry);
                last_project_files = files;
                rt
            } else {
                source = std::fs::read_to_string(p)?;
                template = crate::BUTTON_COMPONENT.into();
                match crate::Runtime::from_sources(&source, &template) {
                    Ok(rt) => rt,
                    Err(_) => {
                        let (root, entry) = resolve_ui_project_root_and_entry(p)
                            .ok_or("Не удалось определить корень .ui проекта")?;
                        let files = collect_ui_project_files(&root)?;
                        let rt = crate::Runtime::from_ui_files(
                            &files,
                            &entry,
                            &crate::form_document::Properties::new(),
                        )
                        .map_err(std::io::Error::other)?;
                        project_root = Some(root);
                        project_entry = Some(entry);
                        last_project_files = files;
                        rt
                    }
                }
            }
        } else {
            source = source_path
                .as_deref()
                .map(std::fs::read_to_string)
                .transpose()?
                .unwrap_or(crate::TWO_BUTTONS_EXAMPLE.into());
            template = template_path
                .as_deref()
                .map(std::fs::read_to_string)
                .transpose()?
                .unwrap_or(crate::BUTTON_COMPONENT.into());
            crate::Runtime::from_sources(&source, &template).map_err(std::io::Error::other)?
        };
        let source_meta = file_stamp(source_path.as_deref());
        let template_meta = file_stamp(template_path.as_deref());
        Ok(Self {
            runtime,
            hot: HotReload {
                source_path: if project_root.is_some() {
                    None
                } else {
                    source_path
                },
                template_path,
                source_meta,
                template_meta,
                project_root,
                project_entry,
                last_project_files,
                last_source: source.clone(),
                last_template: template.clone(),
                next_hot_check: Instant::now() + Duration::from_millis(120),
            },
            source,
            template,
        })
    }
}
