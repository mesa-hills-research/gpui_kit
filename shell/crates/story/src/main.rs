//! Runs the Shell story in a window of its own. It lives here and not in the
//! gallery because the gallery's workspace has no git dependencies.

use gpui_component_story::{StoryContainer, create_new_window, init};
use gpui_kit::assets::AllAssets;

mod shell_story;

fn main() {
    gpui_kit::application().with_assets(AllAssets).run(|cx| {
        init(cx);
        cx.activate(true);
        create_new_window(
            "GPUI Shell",
            |window, cx| StoryContainer::panel::<shell_story::ShellStory>(window, cx),
            cx,
        );
    });
}
