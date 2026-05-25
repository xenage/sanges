use crate::sagens::ui::{BadgeStyle, Theme};

use super::super::HelpTopic;
use super::super::help::PageSpec;

pub(super) fn page_for<'a>(topic: HelpTopic, theme: &'a Theme) -> Option<PageSpec<'a>> {
    match topic {
        HelpTopic::Image => Some(image_page(theme)),
        HelpTopic::ImageBuild => Some(image_build_page(theme)),
        HelpTopic::ImageList => Some(image_list_page(theme)),
        HelpTopic::ImageInspect => Some(image_inspect_page(theme)),
        HelpTopic::ImageRemove => Some(image_remove_page(theme)),
        _ => None,
    }
}

fn image_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens image",
        about: "Build and manage local VM images with shared package caches.",
        badge: Some(theme.badge("Images", BadgeStyle::Info)),
        usage: &["sagens image <command>"],
        commands: &[
            (
                "build",
                "Build a named image from APK, pip, and npm inputs.",
            ),
            ("list", "List locally built images."),
            ("inspect", "Show image metadata."),
            ("rm", "Remove a local image."),
        ],
        examples: &[
            "sagens image build chromium --apk chromium",
            "sagens image list",
            "sagens image inspect chromium",
            "sagens image rm chromium",
        ],
        notes: &[
            "Images are stored under the resolved state directory and can be used with `sagens box new --image NAME`.",
        ],
    }
}

fn image_build_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens image build",
        about: "Build a named VM image and read-only package cache disk.",
        badge: Some(theme.badge("Build", BadgeStyle::Success)),
        usage: &[
            "sagens image build <NAME> [--apk PKG]... [--pip REQ]... [--npm PKG]... [--min-image-mib N] [--force-refresh]",
        ],
        commands: &[],
        examples: &[
            "sagens image build chromium --apk chromium",
            "sagens image build pydeps --pip packaging==25.0",
            "sagens image build nodedeps --npm is-number@7.0.0",
        ],
        notes: &[
            "The image rootfs contains APK packages; pip and npm caches are mounted read-only at `/opt/sagens-cache`.",
        ],
    }
}

fn image_list_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens image list",
        about: "List locally built VM images.",
        badge: Some(theme.badge("Read-only", BadgeStyle::Success)),
        usage: &["sagens image list"],
        commands: &[],
        examples: &["sagens image list"],
        notes: &[],
    }
}

fn image_inspect_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens image inspect",
        about: "Show metadata for a local VM image.",
        badge: Some(theme.badge("Read-only", BadgeStyle::Success)),
        usage: &["sagens image inspect <NAME>"],
        commands: &[],
        examples: &["sagens image inspect chromium"],
        notes: &[],
    }
}

fn image_remove_page(theme: &Theme) -> PageSpec<'_> {
    PageSpec {
        title: "sagens image rm",
        about: "Remove a local VM image.",
        badge: Some(theme.badge("Destructive", BadgeStyle::Danger)),
        usage: &["sagens image rm <NAME>"],
        commands: &[],
        examples: &["sagens image rm chromium"],
        notes: &["The reserved `base` image cannot be removed."],
    }
}
