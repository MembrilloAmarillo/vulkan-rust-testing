use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProgramRunnerError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("toml deserialize error: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("toml serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("program with id '{0}' not found")]
    ProgramNotFound(String),

    #[error("program is already running: {0}")]
    AlreadyRunning(String),

    #[error("process handle {0} not found")]
    HandleNotFound(u32),

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
}

pub type Result<T> = std::result::Result<T, ProgramRunnerError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationInfo {
    pub app_name: String,
    pub app_version: String,
    pub last_opened_unix: u64,
}

impl Default for ApplicationInfo {
    fn default() -> Self {
        Self {
            app_name: "AutomationWare".to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            last_opened_unix: unix_now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramRunnerConfig {
    pub application: ApplicationInfo,
    pub programs: Vec<ProgramConfig>,
}

impl Default for ProgramRunnerConfig {
    fn default() -> Self {
        Self {
            application: ApplicationInfo::default(),
            programs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramConfig {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub auto_start: bool,
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub last_run_unix: Option<u64>,
    #[serde(default)]
    pub last_exit_code: Option<i32>,
}

impl ProgramConfig {
    fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() {
            return Err(ProgramRunnerError::InvalidConfig(
                "program id cannot be empty".to_string(),
            ));
        }

        if self.command.trim().is_empty() {
            return Err(ProgramRunnerError::InvalidConfig(format!(
                "program '{}' command cannot be empty",
                self.id
            )));
        }

        match &self.runtime {
            RuntimeConfig::Direct => {}
            RuntimeConfig::PythonVenv { venv_path } => {
                if venv_path.trim().is_empty() {
                    return Err(ProgramRunnerError::InvalidConfig(format!(
                        "program '{}' has empty Python venv path",
                        self.id
                    )));
                }
            }
            RuntimeConfig::CondaEnv { env_name } => {
                if env_name.trim().is_empty() {
                    return Err(ProgramRunnerError::InvalidConfig(format!(
                        "program '{}' has empty conda environment name",
                        self.id
                    )));
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeConfig {
    Direct,
    PythonVenv { venv_path: String },
    CondaEnv { env_name: String },
}

#[derive(Debug, Clone)]
pub struct ProcessStatus {
    pub handle: u32,
    pub program_id: String,
    pub exited: bool,
    pub exit_code: Option<i32>,
}

#[derive(Debug)]
struct RunningProcess {
    handle: u32,
    program_id: String,
    child: Child,
}

#[derive(Debug)]
pub struct ProgramRunner {
    config_path: PathBuf,
    pub config: ProgramRunnerConfig,
    running: HashMap<u32, RunningProcess>,
    next_handle: u32,
}

impl ProgramRunner {
    pub fn load_or_create(config_path: impl AsRef<Path>) -> Result<Self> {
        let config_path = config_path.as_ref().to_path_buf();

        if let Some(parent) = config_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let mut config = if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            if content.trim().is_empty() {
                ProgramRunnerConfig::default()
            } else {
                toml::from_str::<ProgramRunnerConfig>(&content)?
            }
        } else {
            ProgramRunnerConfig::default()
        };

        config.application.last_opened_unix = unix_now();
        validate_unique_ids(&config.programs)?;
        for program in &config.programs {
            program.validate()?;
        }

        let mut runner = Self {
            config_path,
            config,
            running: HashMap::new(),
            next_handle: 1,
        };

        runner.save()?;
        Ok(runner)
    }

    pub fn save(&mut self) -> Result<()> {
        self.config.application.last_opened_unix = unix_now();
        let text = toml::to_string_pretty(&self.config)?;
        fs::write(&self.config_path, text)?;
        Ok(())
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn list_programs(&self) -> &[ProgramConfig] {
        &self.config.programs
    }

    pub fn upsert_program(&mut self, program: ProgramConfig) -> Result<()> {
        program.validate()?;
        if let Some(existing) = self.config.programs.iter_mut().find(|p| p.id == program.id) {
            *existing = program;
        } else {
            self.config.programs.push(program);
        }
        validate_unique_ids(&self.config.programs)?;
        self.save()
    }

    pub fn remove_program(&mut self, program_id: &str) -> Result<bool> {
        let before = self.config.programs.len();
        self.config.programs.retain(|p| p.id != program_id);
        let removed = self.config.programs.len() != before;
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    pub fn start_program(&mut self, program_id: &str) -> Result<u32> {
        if self.running.values().any(|rp| rp.program_id == program_id) {
            return Err(ProgramRunnerError::AlreadyRunning(program_id.to_string()));
        }

        let program = self
            .config
            .programs
            .iter()
            .find(|p| p.id == program_id)
            .cloned()
            .ok_or_else(|| ProgramRunnerError::ProgramNotFound(program_id.to_string()))?;

        let mut cmd = Self::build_command(&program);

        if let Some(dir) = &program.working_dir {
            cmd.current_dir(dir);
        }

        if !program.env.is_empty() {
            cmd.envs(program.env.clone());
        }

        // Keep output visible in the parent process so failures are visible in the terminal.
        // When running as a GUI app without a terminal, consider setting up a log file instead.
        let child = cmd
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        let handle = self.next_handle;
        self.next_handle = self.next_handle.saturating_add(1);

        self.running.insert(
            handle,
            RunningProcess {
                handle,
                program_id: program.id.clone(),
                child,
            },
        );

        if let Some(cfg) = self.config.programs.iter_mut().find(|p| p.id == program.id) {
            cfg.last_run_unix = Some(unix_now());
            cfg.last_exit_code = None;
            self.save()?;
        }

        Ok(handle)
    }

    pub fn start_auto_programs(&mut self) -> Result<Vec<u32>> {
        let auto_ids: Vec<String> = self
            .config
            .programs
            .iter()
            .filter(|p| p.auto_start)
            .map(|p| p.id.clone())
            .collect();

        let mut handles = Vec::new();
        for id in auto_ids {
            if self.running.values().any(|rp| rp.program_id == id) {
                continue;
            }
            handles.push(self.start_program(&id)?);
        }

        Ok(handles)
    }

    pub fn stop_program(&mut self, handle: u32) -> Result<()> {
        let mut running = self
            .running
            .remove(&handle)
            .ok_or(ProgramRunnerError::HandleNotFound(handle))?;

        running.child.kill()?;
        let status = running.child.wait()?;
        self.update_last_exit_code(&running.program_id, status.code())?;
        Ok(())
    }

    pub fn stop_all(&mut self) -> Result<()> {
        let handles: Vec<u32> = self.running.keys().copied().collect();
        for handle in handles {
            self.stop_program(handle)?;
        }
        Ok(())
    }

    pub fn poll(&mut self) -> Result<Vec<ProcessStatus>> {
        let handles: Vec<u32> = self.running.keys().copied().collect();
        let mut updates = Vec::new();
        let mut finished = Vec::new();

        for handle in handles {
            if let Some(running) = self.running.get_mut(&handle) {
                match running.child.try_wait()? {
                    Some(status) => {
                        let exit_code = status.code();
                        updates.push(ProcessStatus {
                            handle,
                            program_id: running.program_id.clone(),
                            exited: true,
                            exit_code,
                        });
                        finished.push((handle, running.program_id.clone(), exit_code));
                    }
                    None => {
                        updates.push(ProcessStatus {
                            handle,
                            program_id: running.program_id.clone(),
                            exited: false,
                            exit_code: None,
                        });
                    }
                }
            }
        }

        for (handle, program_id, exit_code) in finished {
            self.running.remove(&handle);
            self.update_last_exit_code(&program_id, exit_code)?;
        }

        Ok(updates)
    }

    pub fn running_programs(&self) -> Vec<(u32, &str)> {
        self.running
            .values()
            .map(|r| (r.handle, r.program_id.as_str()))
            .collect()
    }

    fn update_last_exit_code(&mut self, program_id: &str, exit_code: Option<i32>) -> Result<()> {
        if let Some(cfg) = self.config.programs.iter_mut().find(|p| p.id == program_id) {
            cfg.last_exit_code = exit_code;
            cfg.last_run_unix = Some(unix_now());
            self.save()?;
        }
        Ok(())
    }

    fn build_command(program: &ProgramConfig) -> Command {
        match &program.runtime {
            RuntimeConfig::Direct => {
                let mut cmd = Command::new(&program.command);
                if !program.args.is_empty() {
                    cmd.args(&program.args);
                }
                cmd
            }

            RuntimeConfig::PythonVenv { venv_path } => {
                let mut command_string = format!(
                    "source {}/bin/activate && {}",
                    venv_path.trim_end_matches('/'),
                    program.command.trim()
                );

                if !program.args.is_empty() {
                    command_string.push(' ');
                    command_string.push_str(&program.args.join(" "));
                }

                let mut cmd = Command::new("bash");
                cmd.arg("-lc");
                cmd.arg(command_string);
                cmd
            }

            RuntimeConfig::CondaEnv { env_name } => {
                let mut cmd = Command::new("conda");
                cmd.arg("run");
                cmd.arg("-n");
                cmd.arg(env_name);
                cmd.arg(&program.command);
                if !program.args.is_empty() {
                    cmd.args(&program.args);
                }
                cmd
            }
        }
    }
}

fn default_venv_python_path(venv_path: &str) -> String {
    let mut path = PathBuf::from(venv_path);
    if cfg!(windows) {
        path.push("Scripts");
        path.push("python.exe");
    } else {
        path.push("bin");
        path.push("python");
    }
    path.to_string_lossy().to_string()
}

fn validate_unique_ids(programs: &[ProgramConfig]) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for p in programs {
        if !seen.insert(p.id.clone()) {
            return Err(ProgramRunnerError::InvalidConfig(format!(
                "duplicate program id found: {}",
                p.id
            )));
        }
    }
    Ok(())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
