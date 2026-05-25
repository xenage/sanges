use super::super::BoxApiClient;
use crate::Result;
use crate::box_api::protocol::{BoxRequest, BoxResponse};
use crate::images::VmImageManifest;

impl BoxApiClient {
    pub async fn build_image(
        &self,
        name: String,
        apk: Vec<String>,
        pip: Vec<String>,
        npm: Vec<String>,
        min_image_mib: u64,
        force_refresh: bool,
    ) -> Result<VmImageManifest> {
        let request_id = self.next_request_id();
        self.request_response(
            BoxRequest::ImageBuild {
                request_id,
                name,
                apk,
                pip,
                npm,
                min_image_mib,
                force_refresh,
            },
            image_response,
        )
        .await
    }

    pub async fn list_images(&self) -> Result<Vec<VmImageManifest>> {
        let request_id = self.next_request_id();
        self.request_response(BoxRequest::ImageList { request_id }, |response| {
            if let BoxResponse::ImageList { images } = response {
                return Some(images);
            }
            None
        })
        .await
    }

    pub async fn inspect_image(&self, name: String) -> Result<VmImageManifest> {
        let request_id = self.next_request_id();
        self.request_response(
            BoxRequest::ImageInspect { request_id, name },
            image_response,
        )
        .await
    }

    pub async fn remove_image(&self, name: String) -> Result<()> {
        let request_id = self.next_request_id();
        self.request_response(BoxRequest::ImageRemove { request_id, name }, |response| {
            if let BoxResponse::ImageRemoved { .. } = response {
                return Some(());
            }
            None
        })
        .await
    }
}

fn image_response(response: BoxResponse) -> Option<VmImageManifest> {
    if let BoxResponse::Image { image } = response {
        return Some(image);
    }
    None
}
