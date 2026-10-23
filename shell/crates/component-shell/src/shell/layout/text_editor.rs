use super::bool_method;
use std::sync::Arc;

use gpui_component::input::{TextEditor, TextareaState};
use gpui_shell::{
    ArgumentDescriptor, ArgumentSchema, ComponentArgument, ComponentDescriptor,
    ComponentMaterializer, ComponentPayload, ComponentRegistry, ConstructorDescriptor,
    MaterializeRequest, MethodDescriptor, RegistryError, anyhow,
    gpui::{self, Entity, IntoElement as _, Refineable as _, Styled as _},
};

#[derive(Clone)]
enum Op {
    Bordered(bool),
    Readonly(bool),
    AriaLabel(String),
}

fn require_leaf(children: usize) -> anyhow::Result<()> {
    anyhow::ensure!(children == 0, "TextEditor does not accept children");
    Ok(())
}

struct Materializer;
impl ComponentMaterializer for Materializer {
    fn materialize(&self, mut request: MaterializeRequest<'_>) -> anyhow::Result<gpui::AnyElement> {
        let argument = request
            .payload()
            .downcast_ref::<ComponentArgument>()
            .ok_or_else(|| anyhow::anyhow!("TextEditor received an incompatible payload"))?;
        let state = request.with_state::<Entity<TextareaState>, _>(argument, Clone::clone)?;
        let mut editor = TextEditor::new(&state).disabled(request.disabled());
        for op in request
            .methods()
            .filter_map(|method| method.payload().downcast_ref::<Op>())
        {
            editor = match op {
                Op::Bordered(value) => editor.bordered(*value),
                Op::Readonly(value) => editor.readonly(*value),
                Op::AriaLabel(value) => editor.aria_label(value.clone()),
            };
        }
        require_leaf(request.children_len())?;
        editor.style().refine(&request.take_style());
        Ok(editor.into_any_element())
    }
}

pub(super) fn register(registry: &mut ComponentRegistry) -> Result<(), RegistryError> {
    registry.register(
        ComponentDescriptor::new("TextEditor", Arc::new(Materializer))
            .with_constructors(vec![ConstructorDescriptor::new(
                "TextEditor",
                vec![ArgumentDescriptor::new(
                    "state",
                    ArgumentSchema::Entity("TextareaState"),
                )],
                |args| match args {
                    [argument @ ComponentArgument::Entity { .. }] => {
                        Ok(ComponentPayload::new(argument.clone()))
                    }
                    _ => Err("TextEditor expects one TextareaState entity".into()),
                },
            )])
            .with_methods(vec![
                MethodDescriptor::new(
                    "disabled",
                    vec![ArgumentDescriptor::new("disabled", ArgumentSchema::Boolean)],
                    |_| Ok(ComponentPayload::new(())),
                )
                .with_documentation("Sets the common disabled state."),
                bool_method(
                    "TextEditor",
                    "bordered",
                    "Draws a border around the editor. Off by default.",
                    Op::Bordered,
                ),
                bool_method(
                    "TextEditor",
                    "readonly",
                    "Lets the text be selected and copied but not edited.",
                    Op::Readonly,
                ),
                MethodDescriptor::new(
                    "aria_label",
                    vec![ArgumentDescriptor::new("label", ArgumentSchema::String)],
                    |args| match args {
                        [ComponentArgument::String(value)] if !value.trim().is_empty() => {
                            Ok(ComponentPayload::new(Op::AriaLabel(value.clone())))
                        }
                        _ => Err("TextEditor.aria_label expects non-empty text".into()),
                    },
                )
                .with_documentation("Sets the accessibility label."),
            ])
            .with_documentation(
                "A retained textarea laid out as a text editor: it fills its parent's height, \
                 borderless, on the theme's editor background. Create its state with \
                 `TextareaState(text, true)` for line numbers, the search panel and soft wrap. \
                 Shell style and common disabled state are honored; children are rejected.",
            ),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_editor_is_an_exact_leaf() {
        assert!(require_leaf(0).is_ok());
        assert_eq!(
            require_leaf(1).unwrap_err().to_string(),
            "TextEditor does not accept children"
        );
    }
}
