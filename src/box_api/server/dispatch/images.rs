use super::super::{ImageApiConfig, WsWriter};
use super::send_response;
use crate::box_api::protocol::{BoxRequest, BoxResponse};
use crate::images::{ImageBuildSpec, ImageStore};
use crate::{Result, SandboxError};

pub(super) async fn dispatch_image_request(
    writer: &WsWriter,
    config: ImageApiConfig,
    request: BoxRequest,
) -> Result<()> {
    match request {
        BoxRequest::ImageBuild {
            request_id,
            name,
            apk,
            pip,
            npm,
            min_image_mib,
            force_refresh,
        } => {
            build_image(
                writer,
                config,
                request_id,
                name,
                apk,
                pip,
                npm,
                min_image_mib,
                force_refresh,
            )
            .await
        }
        BoxRequest::ImageList { request_id } => list_images(writer, config, request_id).await,
        BoxRequest::ImageInspect { request_id, name } => {
            inspect_image(writer, config, request_id, name).await
        }
        BoxRequest::ImageRemove { request_id, name } => {
            remove_image(writer, config, request_id, name).await
        }
        _ => unreachable!("non-image request routed to image dispatcher"),
    }
}

#[allow(clippy::too_many_arguments)]
async fn build_image(
    writer: &WsWriter,
    config: ImageApiConfig,
    request_id: String,
    name: String,
    apk: Vec<String>,
    pip: Vec<String>,
    npm: Vec<String>,
    min_image_mib: u64,
    force_refresh: bool,
) -> Result<()> {
    let store = ImageStore::new(&config.state_dir);
    let spec = ImageBuildSpec {
        state_dir: config.state_dir,
        name,
        apk,
        pip,
        npm,
        min_image_mib,
        force_refresh,
        base_guest: config.base_guest,
    };
    let image = tokio::task::spawn_blocking(move || store.build(spec))
        .await
        .map_err(|error| SandboxError::backend(format!("joining image build: {error}")))??;
    send_response(writer, request_id, BoxResponse::Image { image }).await
}

async fn list_images(writer: &WsWriter, config: ImageApiConfig, request_id: String) -> Result<()> {
    let images = ImageStore::new(config.state_dir).list()?;
    send_response(writer, request_id, BoxResponse::ImageList { images }).await
}

async fn inspect_image(
    writer: &WsWriter,
    config: ImageApiConfig,
    request_id: String,
    name: String,
) -> Result<()> {
    let image = ImageStore::new(config.state_dir).inspect(&name)?;
    send_response(writer, request_id, BoxResponse::Image { image }).await
}

async fn remove_image(
    writer: &WsWriter,
    config: ImageApiConfig,
    request_id: String,
    name: String,
) -> Result<()> {
    ImageStore::new(config.state_dir).remove(&name)?;
    send_response(writer, request_id, BoxResponse::ImageRemoved { name }).await
}
