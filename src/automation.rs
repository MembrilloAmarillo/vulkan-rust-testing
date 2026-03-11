//! Automation file loader module
//!
//! Provides UI components for loading and displaying automation scripts/configurations.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

#[derive(Debug)]
struct LoadRequest {
    id: u64,
    path: PathBuf,
}

#[derive(Debug)]
struct LoadResult {
    id: u64,
    path: PathBuf,
    content: Result<(String, Vec<usize>), String>,
}

/// State for the automation file loader UI
#[derive(Debug)]
pub struct AutomationFileLoader {
    /// Current directory being browsed
    pub current_dir: PathBuf,
    /// List of files in current directory
    pub files: Vec<FileEntry>,
    /// Currently selected file path
    pub selected_file: Option<PathBuf>,
    /// Content of the selected file
    pub file_content: Option<String>,
    /// Error message if any
    pub error_message: Option<String>,
    /// Byte offsets for each line start; last entry is file length.
    pub file_line_starts: Option<Vec<usize>>,
    /// Show file browser window
    pub show_browser: bool,
    /// Show code display window
    pub show_code_display: bool,
    /// Filter by file extension (e.g., ".lua", ".json", ".rs")
    pub file_filter: String,
    /// Tracks whether an async file-load request is in flight
    pub is_loading_file: bool,
    pending_request_id: Option<u64>,
    next_request_id: u64,
    load_tx: Sender<LoadRequest>,
    load_rx: Receiver<LoadResult>,
}

/// Represents a single file entry
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub display_label: String,
}

impl Default for AutomationFileLoader {
    fn default() -> Self {
        let (load_tx, request_rx) = mpsc::channel::<LoadRequest>();
        let (result_tx, load_rx) = mpsc::channel::<LoadResult>();
        std::thread::spawn(move || {
            while let Ok(request) = request_rx.recv() {
                let content = std::fs::read_to_string(&request.path)
                    .map(|content| {
                        let mut line_starts = Vec::with_capacity(1024);
                        line_starts.push(0);
                        for (idx, byte) in content.as_bytes().iter().enumerate() {
                            if *byte == b'\n' {
                                line_starts.push(idx + 1);
                            }
                        }
                        if line_starts.last().copied().unwrap_or(0) != content.len() {
                            line_starts.push(content.len());
                        }
                        (content, line_starts)
                    })
                    .map_err(|e| format!("Failed to load file: {}", e));
                let _ = result_tx.send(LoadResult {
                    id: request.id,
                    path: request.path,
                    content,
                });
            }
        });

        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        AutomationFileLoader {
            current_dir,
            files: Vec::new(),
            selected_file: None,
            file_content: None,
            error_message: None,
            file_line_starts: None,
            show_browser: true,
            show_code_display: false,
            file_filter: String::new(), // Empty = show all files
            is_loading_file: false,
            pending_request_id: None,
            next_request_id: 1,
            load_tx,
            load_rx,
        }
    }
}

impl AutomationFileLoader {
    /// Creates a new file loader with optional starting directory
    pub fn new(start_dir: Option<PathBuf>) -> Self {
        let mut loader = AutomationFileLoader::default();
        if let Some(dir) = start_dir {
            loader.current_dir = dir;
        }
        loader.refresh_files();
        loader
    }

    /// Refreshes the file list from the current directory
    pub fn refresh_files(&mut self) {
        self.files.clear();
        self.error_message = None;

        // Try to read the directory
        match std::fs::read_dir(&self.current_dir) {
            Ok(entries) => {
                let mut file_list: Vec<FileEntry> = entries
                    .filter_map(|entry| {
                        let entry = entry.ok()?;
                        let path = entry.path();
                        let name = entry.file_name().to_string_lossy().to_string();
                        let file_type = entry.file_type().ok()?;
                        let is_dir = file_type.is_dir();

                        // Apply filter if set
                        if !self.file_filter.is_empty() && !is_dir {
                            if !name.ends_with(&self.file_filter) {
                                return None;
                            }
                        }

                        let size = if is_dir {
                            None
                        } else {
                            entry.metadata().ok().map(|m| m.len())
                        };

                        let icon = if is_dir { "📁" } else { "📄" };
                        let size_suffix = size
                            .map(|bytes| format!(" ({})", Self::format_size(bytes)))
                            .unwrap_or_default();
                        let display_label = format!("{} {}{}", icon, name, size_suffix);

                        Some(FileEntry {
                            path,
                            name,
                            is_dir,
                            size,
                            display_label,
                        })
                    })
                    .collect();

                // Sort: directories first, then alphabetically
                file_list.sort_by(|a, b| match (b.is_dir, a.is_dir) {
                    (true, false) => std::cmp::Ordering::Greater,
                    (false, true) => std::cmp::Ordering::Less,
                    _ => a.name.cmp(&b.name),
                });

                self.files = file_list;
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to read directory: {}", e));
            }
        }
    }

    /// Navigate to a subdirectory
    pub fn navigate_to(&mut self, path: &Path) {
        if path.is_dir() {
            self.current_dir = path.to_path_buf();
            self.refresh_files();
            self.selected_file = None;
            self.file_content = None;
            self.file_line_starts = None;
        }
    }

    /// Navigate to parent directory
    pub fn navigate_up(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            self.current_dir = parent.to_path_buf();
            self.refresh_files();
            self.selected_file = None;
            self.file_content = None;
            self.file_line_starts = None;
        }
    }

    /// Load file content
    pub fn load_file(&mut self, path: &Path) {
        self.error_message = None;

        if !path.is_file() {
            self.error_message = Some("Not a file".to_string());
            return;
        }

        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        self.pending_request_id = Some(request_id);
        self.is_loading_file = true;
        self.selected_file = Some(path.to_path_buf());
        self.show_code_display = true;
        self.file_content = None;
        self.file_line_starts = None;

        if let Err(e) = self.load_tx.send(LoadRequest {
            id: request_id,
            path: path.to_path_buf(),
        }) {
            self.is_loading_file = false;
            self.pending_request_id = None;
            self.error_message = Some(format!("Failed to queue file load: {}", e));
        }
    }

    /// Poll completion of async file-load requests.
    pub fn poll_file_load(&mut self) {
        while let Ok(result) = self.load_rx.try_recv() {
            if self.pending_request_id != Some(result.id) {
                continue;
            }

            self.is_loading_file = false;
            self.pending_request_id = None;
            match result.content {
                Ok((content, line_starts)) => {
                    self.selected_file = Some(result.path);
                    self.file_content = Some(content);
                    self.file_line_starts = Some(line_starts);
                    self.show_code_display = true;
                    self.error_message = None;
                }
                Err(e) => {
                    self.file_content = None;
                    self.file_line_starts = None;
                    self.error_message = Some(e);
                }
            }
        }
    }

    /// Set file extension filter (e.g., ".lua", ".json")
    pub fn set_filter(&mut self, filter: String) {
        self.file_filter = filter;
        self.refresh_files();
    }

    /// Get currently loaded file name
    pub fn current_file_name(&self) -> Option<String> {
        self.selected_file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
    }

    /// Get file size in human-readable format
    pub fn format_size(bytes: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
        let mut size = bytes as f64;
        let mut unit_idx = 0;

        while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
            size /= 1024.0;
            unit_idx += 1;
        }

        if unit_idx == 0 {
            format!("{} {}", size as u64, UNITS[unit_idx])
        } else {
            format!("{:.2} {}", size, UNITS[unit_idx])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(AutomationFileLoader::format_size(512), "512 B");
        assert_eq!(AutomationFileLoader::format_size(1024), "1.00 KB");
        assert_eq!(AutomationFileLoader::format_size(1024 * 1024), "1.00 MB");
    }

    #[test]
    fn test_default_loader() {
        let loader = AutomationFileLoader::default();
        assert!(loader.current_dir.is_absolute() || !loader.current_dir.as_os_str().is_empty());
        assert!(loader.selected_file.is_none());
        assert!(loader.file_content.is_none());
    }

    #[test]
    fn test_filter_files() {
        let temp_dir = std::env::temp_dir();
        let mut loader = AutomationFileLoader::new(Some(temp_dir));
        loader.set_filter(".txt".to_string());
        // After filtering, only .txt files should appear
        for file in &loader.files {
            if !file.is_dir {
                assert!(file.name.ends_with(".txt"));
            }
        }
    }
}
