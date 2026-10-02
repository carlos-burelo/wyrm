use std::env;
use std::ffi::OsString;
use std::time::Duration;
use windows_service::{
    define_windows_service,
    service::{
        ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
        ServiceErrorControl, ServiceExitCode, ServiceInfo, ServiceStartType, ServiceState,
        ServiceStatus, ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
    service_manager::{ServiceManager, ServiceManagerAccess},
};

pub const SERVICE_NAME: &str = "WyrmDaemon";
pub const DISPLAY_NAME: &str = "Wyrm Process Manager Service";

define_windows_service!(ffi_service_main, my_service_main);

pub fn install_service() -> Result<(), Box<dyn std::error::Error>> {
    let current_exe = env::current_exe()?;
    let manager = ServiceManager::local_computer(
        None,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )?;

    let service_info = ServiceInfo {
        name: SERVICE_NAME,
        display_name: DISPLAY_NAME,
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::Auto,
        error_control: ServiceErrorControl::Normal,
        executable_path: current_exe.as_path(),
        launch_arguments: vec!["--daemon"],
        dependencies: vec![],
        account_name: None,
        password: None,
    };

    let service = manager.create_service(&service_info, ServiceAccess::SET_FAILURE_ACTIONS)?;

    let actions = vec![
        ServiceAction {
            action_type: ServiceActionType::Restart,
            delay: Duration::from_secs(5),
        },
        ServiceAction {
            action_type: ServiceActionType::Restart,
            delay: Duration::from_secs(10),
        },
    ];

    service.set_failure_actions(actions, Duration::from_secs(86400), None, None)?;

    println!("Servicio {} instalado y configurado para arranque automático.", SERVICE_NAME);
    Ok(())
}

pub fn start_service_dispatcher() -> Result<(), windows_service::Error> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
}

fn my_service_main(_arguments: Vec<OsString>) {
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    if let Ok(status_handle) = service_control_handler::register(SERVICE_NAME, event_handler) {
        let _ = status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP,
            exit_code: ServiceExitCode::NoError,
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        });

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let _ = crate::ipc::run_ipc_server().await;
        });

        let _ = status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::NoError,
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        });
    }
}