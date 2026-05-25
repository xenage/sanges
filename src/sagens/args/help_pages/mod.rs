mod box_pages;
mod general;
mod image;

use crate::sagens::ui::Theme;

use super::HelpTopic;
use super::help::PageSpec;

pub(super) fn page_for<'a>(topic: HelpTopic, theme: &'a Theme) -> PageSpec<'a> {
    if let Some(page) = general::page_for(topic, theme) {
        return page;
    }
    if let Some(page) = image::page_for(topic, theme) {
        return page;
    }
    box_pages::page_for(topic, theme)
}
