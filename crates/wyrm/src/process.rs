use std::fs::OpenOptions;
use std::path::Path;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

pub(crate) struct JobHandle(HANDLE);

impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

// SAFETY: el HANDLE del Job solo se usa para AssignProcessToJobObject desde el
// hilo que hace spawn. El daemon es single-owner por app.
unsafe impl Send for JobHandle {}
unsafe impl Sync for JobHandle {}

pub struct ManagedChild {
    pub child: tokio::process::Child,
    _job: JobHandle,
}

impl ManagedChild {
    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }
}

pub(crate) fn create_job() -> Result<JobHandle, Box<dyn std::error::Error + Send + Sync>> {
    unsafe {
        let job = CreateJobObjectW(None, None)?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )?;
        Ok(JobHandle(job))
    }
}

pub fn spawn_managed(
    config: &crate::inspector::AppConfig,
    log_path: &Path,
) -> Result<ManagedChild, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    let err_file = log_file.try_clone()?;

    let mut cmd = tokio::process::Command::new(&config.executable);
    cmd.args(&config.args)
        .current_dir(&config.cwd)
        .envs(&config.env)
        .stdout(log_file)
        .stderr(err_file)
        .kill_on_drop(true);

    // Evita ventana de consola en Windows Server.
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let child = cmd.spawn()?;
    let job = create_job()?;
    unsafe {
        // tokio 1.x: raw_handle() -> Option<RawHandle>
        if let Some(raw) = child.raw_handle() {
            let process_handle = HANDLE(raw as *mut std::ffi::c_void);
            AssignProcessToJobObject(job.0, process_handle)?;
        }
    }

    Ok(ManagedChild { child, _job: job })
}
