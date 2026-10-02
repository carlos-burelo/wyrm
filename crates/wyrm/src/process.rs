use std::os::windows::io::AsRawHandle;
use std::process::{Child, Command, Stdio};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JobObjectExtendedLimitInformation,
};

pub struct ProcessGuard {
    job_handle: HANDLE,
}

impl ProcessGuard {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
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

            Ok(Self { job_handle: job })
        }
    }

    pub fn spawn_managed(&self, config: &crate::inspector::AppConfig) -> Result<Child, Box<dyn std::error::Error>> {
        let mut cmd = Command::new(&config.executable);
        cmd.args(&config.args)
           .current_dir(&config.cwd)
           .envs(&config.env)
           .stdout(Stdio::piped())
           .stderr(Stdio::piped());

        let child = cmd.spawn()?;
        unsafe {
            let process_handle = HANDLE(child.as_raw_handle() as isize);
            AssignProcessToJobObject(self.job_handle, process_handle)?;
        }

        Ok(child)
    }
}