use std::env;
use std::ffi::OsString;
use std::time::Duration;
use windows_service::{
    define_windows_service,
    service::{
        ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
        ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
        ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
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
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .map_err(|e| format!("abriendo Service Manager (¿consola elevada?): {e}"))?;

    let service_info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: current_exe,
        launch_arguments: vec![OsString::from("--daemon")],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };

    let service = manager
        .create_service(&service_info, ServiceAccess::CHANGE_CONFIG)
        .map_err(|e| {
            // 1072 = borrado pendiente: el demonio viejo sigue vivo o hay
            // handles abiertos (services.msc). Sin cerrar eso no hay reinstall.
            // (El código solo aparece en el Debug del error Winapi.)
            if format!("{e:?}").contains("1072") {
                format!("creando servicio {SERVICE_NAME}: marcado para borrado (1072). Mata el demonio viejo con `taskkill /F /IM wyrm.exe`, cierra services.msc y reintenta; si persiste, reinicia.")
            } else {
                format!("creando servicio {SERVICE_NAME}: {e}")
            }
        })?;

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

    // El recovery no es fatal: en algunos entornos el SCM lo deniega aun
    // como admin (código 5). El servicio queda instalado y funcional; las
    // acciones se pueden fijar con:
    // `sc.exe failure WyrmDaemon reset= 86400 actions= restart/5000/restart/10000`
    if let Err(e) = service.update_failure_actions(ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86400)),
        reboot_msg: None,
        command: None,
        actions: Some(actions),
    }) {
        eprintln!("Aviso: servicio instalado pero sin recovery automático ({e}).");
        eprintln!(
            "Fíjalo manual: sc.exe failure {SERVICE_NAME} reset= 86400 actions= restart/5000/restart/10000"
        );
    }

    println!(
        "Servicio {} instalado y configurado para arranque automático.",
        SERVICE_NAME
    );
    Ok(())
}

pub fn uninstall_service() -> Result<(), Box<dyn std::error::Error>> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = manager.open_service(
        SERVICE_NAME,
        ServiceAccess::DELETE | ServiceAccess::STOP | ServiceAccess::QUERY_STATUS,
    )?;
    let _ = service.stop();
    service.delete()?;
    println!("Servicio {SERVICE_NAME} desinstalado.");
    Ok(())
}

pub fn start_service_dispatcher() -> Result<(), windows_service::Error> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
}

fn my_service_main(_arguments: Vec<OsString>) {
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop => {
                // Apagado real del proceso. Sin esto el SCM nunca completa el
                // Stop: el `uninstall` deja el servicio en "marked for deletion"
                // (1072) y el reinstall falla. Los hijos mueren con el JobObject.
                std::process::exit(0);
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    if let Ok(status_handle) = service_control_handler::register(SERVICE_NAME, event_handler) {
        let _ = status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP,
            exit_code: ServiceExitCode::NO_ERROR,
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        });

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let daemon = crate::daemon::Daemon::new();
            daemon.restore_from_db().await;
            let d2 = daemon.clone();
            tokio::spawn(async move { d2.supervise().await });
            let d3 = daemon.clone();
            tokio::spawn(async move { crate::daemon::health::health_loop(d3).await });
            let d4 = daemon.clone();
            tokio::spawn(async move {
                let _ = crate::api::serve(d4).await;
            });
            let handler = crate::daemon::blocking_handler(daemon);
            let _ = crate::ipc::run_ipc_server_with(handler).await;
        });

        let _ = status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::NO_ERROR,
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        });
    }
}
