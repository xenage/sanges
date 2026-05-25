use crate::auth::read_user_config;
use crate::backend::libkrun::runner;
use crate::sagens::args::{
    AdminCommand, BoxCommand, BoxSetCommand, CheckpointCommand, Command, DaemonCommand, ExecTarget,
    FsCommand, ImageCommand,
};
use crate::sagens::client::{SagensClient, download_path, upload_path};
use crate::sagens::config::{
    build_runtime_config_for_endpoint, resolve_guest_config_for_images, resolve_paths,
};
use crate::sagens::{args, daemon, output, update};
use crate::{Result, SandboxError};

pub async fn run() -> Result<i32> {
    let raw_args = std::env::args().skip(1).collect::<Vec<_>>();
    if let Some(config_path) = parse_internal_libkrun_runner(&raw_args)? {
        runner::run_from_file(&config_path)?;
        return Ok(0);
    }
    if let Some(config_path) = parse_internal_security_harness(&raw_args)? {
        runner::run_security_harness_from_file(&config_path)?;
        return Ok(0);
    }
    let command = args::parse(raw_args)?;
    let paths = resolve_paths();
    match command {
        Command::Help(topic) => {
            output::print_help(&args::render_help(topic))
                .map_err(|error| SandboxError::io("writing help output", error))?;
            Ok(0)
        }
        Command::Start => {
            let host_binary = std::env::current_exe()
                .map_err(|error| SandboxError::io("discovering sagens binary", error))?;
            let runtime_config =
                build_runtime_config_for_endpoint(&paths.state_dir, &paths.endpoint)?;
            let (config, already_running) = daemon::ensure_started(&paths, &host_binary).await?;
            let runtime_info = daemon::read_runtime_info(&paths.state_dir).await?;
            output::print_start_message(
                &config.endpoint,
                already_running,
                runtime_info
                    .map(|info| info.isolation_mode)
                    .unwrap_or(runtime_config.isolation_mode),
            )
            .map_err(|error| SandboxError::io("writing start output", error))?;
            Ok(0)
        }
        Command::Quit => {
            let was_running = daemon::quit(&paths).await?;
            output::print_quit_message(was_running)
                .map_err(|error| SandboxError::io("writing quit output", error))?;
            Ok(0)
        }
        Command::Update => {
            let outcome = update::run_self_update().await?;
            output::print_update_message(&outcome)
                .map_err(|error| SandboxError::io("writing update output", error))?;
            Ok(0)
        }
        Command::Daemon(command) => match command {
            DaemonCommand::Run => {
                let host_binary = std::env::current_exe()
                    .map_err(|error| SandboxError::io("discovering sagens binary", error))?;
                daemon::run_foreground(&paths, &host_binary).await?;
                Ok(0)
            }
            DaemonCommand::Log(command) => {
                daemon::print_log(&paths, command.tail, command.follow).await?;
                Ok(0)
            }
        },
        Command::Admin(command) => run_admin_command(&paths.user_config_path, command).await,
        Command::Image(command) => run_image_command(&paths, command).await,
        Command::Box(command) => run_box_command(&paths, command).await,
    }
}

fn parse_internal_libkrun_runner(args: &[String]) -> Result<Option<std::path::PathBuf>> {
    if args.is_empty() || args[0] != "__libkrun-runner" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(SandboxError::invalid(
            "usage: sagens __libkrun-runner <RUNNER_CONFIG_PATH>",
        ));
    }
    Ok(Some(std::path::PathBuf::from(&args[1])))
}

fn parse_internal_security_harness(args: &[String]) -> Result<Option<std::path::PathBuf>> {
    if args.is_empty() || args[0] != "__security-harness" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(SandboxError::invalid(
            "usage: sagens __security-harness <RUNNER_CONFIG_PATH>",
        ));
    }
    Ok(Some(std::path::PathBuf::from(&args[1])))
}

async fn run_admin_command(
    user_config_path: &std::path::Path,
    command: AdminCommand,
) -> Result<i32> {
    let config = read_user_config(user_config_path)
        .await
        .map_err(|error| SandboxError::backend(format!("{error}; run `sagens start` first")))?;
    let client = SagensClient::connect(&config)
        .await
        .map_err(|error| SandboxError::backend(format!("{error}; run `sagens start` first")))?;
    match command {
        AdminCommand::Add => {
            output::print_admin_bundle(&client.admin_add().await?)
                .map_err(|error| SandboxError::io("writing admin add output", error))?;
        }
        AdminCommand::RemoveMe => {
            client.admin_remove_me().await?;
            output::print_admin_removed()
                .map_err(|error| SandboxError::io("writing admin remove output", error))?;
        }
    }
    Ok(0)
}

async fn run_box_command(
    paths: &crate::sagens::config::SagensPaths,
    command: BoxCommand,
) -> Result<i32> {
    let config = read_user_config(&paths.user_config_path)
        .await
        .map_err(|error| SandboxError::backend(format!("{error}; run `sagens start` first")))?;
    let client = SagensClient::connect(&config)
        .await
        .map_err(|error| SandboxError::backend(format!("{error}; run `sagens start` first")))?;
    match command {
        BoxCommand::List => {
            let runtime_info = daemon::read_runtime_info(&paths.state_dir).await?;
            output::print_box_table(
                &client.list_boxes().await?,
                runtime_info.map(|info| info.isolation_mode),
            )
            .map_err(|error| SandboxError::io("writing box list output", error))?;
            Ok(0)
        }
        BoxCommand::New(command) => {
            output::print_box_action(
                "created",
                &client.create_box_from_image(command.image).await?,
            )
            .map_err(|error| SandboxError::io("writing box create output", error))?;
            Ok(0)
        }
        BoxCommand::Start(box_id) => {
            output::print_box_action("started", &client.start_box(box_id).await?)
                .map_err(|error| SandboxError::io("writing box start output", error))?;
            Ok(0)
        }
        BoxCommand::Stop(box_id) => {
            output::print_box_action("stopped", &client.stop_box(box_id).await?)
                .map_err(|error| SandboxError::io("writing box stop output", error))?;
            Ok(0)
        }
        BoxCommand::Remove(box_id) => {
            client.remove_box(box_id).await?;
            output::print_removed(box_id)
                .map_err(|error| SandboxError::io("writing box remove output", error))?;
            Ok(0)
        }
        BoxCommand::Set(command) => {
            run_box_set_command(&client, command).await?;
            Ok(0)
        }
        BoxCommand::Exec(command) => match command.target {
            ExecTarget::Bash(shell) => client.exec_bash(command.box_id, shell).await,
            ExecTarget::Python(args) => client.exec_python(command.box_id, args).await,
            ExecTarget::Interactive(target) => {
                client.interactive_shell(command.box_id, target).await
            }
        },
        BoxCommand::Fs(command) => run_fs_command(&client, command).await,
        BoxCommand::Checkpoint(command) => run_checkpoint_command(&client, command).await,
    }
}

async fn run_image_command(
    paths: &crate::sagens::config::SagensPaths,
    command: ImageCommand,
) -> Result<i32> {
    let store = crate::images::ImageStore::new(&paths.state_dir);
    match command {
        ImageCommand::Build(command) => {
            let base_guest = resolve_guest_config_for_images(&paths.state_dir).await?;
            let spec = crate::images::ImageBuildSpec {
                state_dir: paths.state_dir.clone(),
                name: command.name,
                apk: command.apk,
                pip: command.pip,
                npm: command.npm,
                min_image_mib: command.min_image_mib,
                force_refresh: command.force_refresh,
                base_guest,
            };
            let manifest = tokio::task::spawn_blocking(move || store.build(spec))
                .await
                .map_err(|error| {
                    SandboxError::backend(format!("joining image build: {error}"))
                })??;
            output::print_image_built(&manifest)
                .map_err(|error| SandboxError::io("writing image build output", error))?;
        }
        ImageCommand::List => {
            output::print_image_list(&store.list()?)
                .map_err(|error| SandboxError::io("writing image list output", error))?;
        }
        ImageCommand::Inspect { name } => {
            output::print_image_inspect(&store.inspect(&name)?)
                .map_err(|error| SandboxError::io("writing image inspect output", error))?;
        }
        ImageCommand::Remove { name } => {
            store.remove(&name)?;
            output::print_image_removed(&name)
                .map_err(|error| SandboxError::io("writing image remove output", error))?;
        }
    }
    Ok(0)
}

async fn run_fs_command(client: &SagensClient, command: FsCommand) -> Result<i32> {
    match command {
        FsCommand::List { box_id, path } => {
            output::print_files(&client.list_files(box_id, path).await?)
                .map_err(|error| SandboxError::io("writing fs list output", error))?;
        }
        FsCommand::Upload {
            box_id,
            local_path,
            remote_path,
        } => {
            upload_path(
                client,
                box_id,
                std::path::Path::new(&local_path),
                std::path::Path::new(&remote_path),
            )
            .await?;
        }
        FsCommand::Download {
            box_id,
            remote_path,
            local_path,
        } => {
            download_path(
                client,
                box_id,
                &remote_path,
                std::path::Path::new(&local_path),
            )
            .await?;
        }
    }
    Ok(0)
}

async fn run_box_set_command(client: &SagensClient, command: BoxSetCommand) -> Result<i32> {
    let box_id = match command.box_id {
        Some(box_id) => box_id,
        None => resolve_single_box_id(client).await?,
    };
    let record = client.set_box_setting(box_id, command.value).await?;
    output::print_box_action("settings updated", &record)
        .map_err(|error| SandboxError::io("writing box set output", error))?;
    Ok(0)
}

async fn run_checkpoint_command(client: &SagensClient, command: CheckpointCommand) -> Result<i32> {
    match command {
        CheckpointCommand::Create {
            box_id,
            name,
            metadata,
        } => {
            let checkpoint = client.checkpoint_create(box_id, name, metadata).await?;
            output::print_checkpoint_id(&checkpoint.summary.checkpoint_id)
                .map_err(|error| SandboxError::io("writing checkpoint create output", error))?;
        }
        CheckpointCommand::List { box_id } => {
            output::print_checkpoints(&client.checkpoint_list(box_id).await?)
                .map_err(|error| SandboxError::io("writing checkpoint list output", error))?;
        }
        CheckpointCommand::Restore {
            box_id,
            checkpoint_id,
            mode,
        } => {
            client
                .checkpoint_restore(box_id, checkpoint_id.clone(), mode)
                .await?;
            output::print_checkpoint_restore_ok(&checkpoint_id)
                .map_err(|error| SandboxError::io("writing checkpoint restore output", error))?;
        }
        CheckpointCommand::Fork {
            box_id,
            checkpoint_id,
            new_box_name,
        } => {
            let record = client
                .checkpoint_fork(box_id, checkpoint_id, new_box_name)
                .await?;
            output::print_box_action("forked", &record)
                .map_err(|error| SandboxError::io("writing checkpoint fork output", error))?;
        }
        CheckpointCommand::Delete {
            box_id,
            checkpoint_id,
        } => {
            client
                .checkpoint_delete(box_id, checkpoint_id.clone())
                .await?;
            output::print_checkpoint_delete_ok(&checkpoint_id)
                .map_err(|error| SandboxError::io("writing checkpoint delete output", error))?;
        }
    }
    Ok(0)
}

async fn resolve_single_box_id(client: &SagensClient) -> Result<uuid::Uuid> {
    let boxes = client.list_boxes().await?;
    match boxes.as_slice() {
        [record] => Ok(record.box_id),
        [] => Err(SandboxError::not_found(
            "no BOXes found; create one first or specify BOX_ID explicitly",
        )),
        _ => Err(SandboxError::invalid(
            "multiple BOXes found; specify BOX_ID explicitly",
        )),
    }
}
