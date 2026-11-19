//! The tao event loop: setup stays in `App::run`, this is the dispatch.
//!
//! Everything that can happen after the first window exists lives here —
//! window bookkeeping, keyboard, and every `UserEvent` another thread can
//! post — so the platform phases (Android's activity callbacks, tao's
//! UIKit suspend/resume mapping) have one match to extend instead of the
//! builder's run method.

use std::sync::Arc;

use serde_json::Value;
use tao::{
    event::{ElementState, Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopWindowTarget},
    keyboard::ModifiersState,
};

use crate::{
    is_devtools_shortcut, open_devtools, windows, AppHandle, BuilderFactory, UserEvent,
};

/// What the event-loop closure carries: the pieces `App::run` assembles
/// before handing control to tao.
pub(crate) struct LoopState {
    pub(crate) open: windows::Windows,
    pub(crate) modifiers: ModifiersState,
    pub(crate) spec: Arc<windows::Spec>,
    pub(crate) factory: BuilderFactory,
    pub(crate) handle: AppHandle,
    pub(crate) show_devtools: bool,
    #[cfg(feature = "menu")]
    pub(crate) menu_commands: std::collections::HashMap<String, String>,
}

/// Handle one event against the shared loop state.
pub(crate) fn dispatch(
    state: &mut LoopState,
    event: Event<'_, UserEvent>,
    event_loop: &EventLoopWindowTarget<UserEvent>,
    control_flow: &mut ControlFlow,
) {
    *control_flow = ControlFlow::Wait;
    match event {
        Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            window_id,
            ..
        } => {
            state.open.remove(&window_id);
            if state.open.is_empty() {
                *control_flow = ControlFlow::Exit;
            }
        }
        Event::WindowEvent {
            event: WindowEvent::ModifiersChanged(mods),
            ..
        } => state.modifiers = mods,
        Event::WindowEvent {
            event:
                WindowEvent::KeyboardInput {
                    event: key,
                    ..
                },
            window_id,
            ..
        } => {
            if state.show_devtools
                && key.state == ElementState::Pressed
                && is_devtools_shortcut(key.physical_key, state.modifiers)
            {
                if let Some(entry) = state.open.get(&window_id) {
                    open_devtools(&entry.webview);
                }
            }
        }
        Event::UserEvent(UserEvent::Emit {
            label,
            event,
            payload,
        }) => {
            let script = format!(
                "window.__kiln_emit&&window.__kiln_emit({},{})",
                serde_json::to_string(&event).unwrap_or_else(|_| "\"\"".to_owned()),
                payload
            );
            for entry in state.open.values() {
                if label.as_ref().is_none_or(|l| *l == entry.label) {
                    if let Err(error) = entry.webview.evaluate_script(&script) {
                        eprintln!("kiln: emit failed: {error}");
                    }
                }
            }
        }
        Event::UserEvent(UserEvent::OpenWindow(options)) => {
            match windows::create_window(event_loop, &state.spec, &options, &*state.factory) {
                Ok((id, entry)) => windows::insert(&mut state.open, id, entry),
                Err(error) => eprintln!("kiln: could not open window: {error}"),
            }
        }
        Event::UserEvent(UserEvent::CloseWindow(label)) => {
            if let Some(id) = state
                .open
                .iter()
                .find(|(_, e)| e.label == label)
                .map(|(id, _)| *id)
            {
                state.open.remove(&id);
                if state.open.is_empty() {
                    *control_flow = ControlFlow::Exit;
                }
            }
        }
        Event::UserEvent(UserEvent::Tray(id)) => {
            state
                .handle
                .emit_to_all("tray", serde_json::json!({ "id": id }));
        }
        Event::UserEvent(UserEvent::Menu(id)) => {
            state
                .handle
                .emit_to_all("menu", serde_json::json!({ "id": id }));
            #[cfg(feature = "menu")]
            if let Some(command) = state.menu_commands.get(&id) {
                state.handle.run_command(command, Value::Null);
            }
            #[cfg(not(feature = "menu"))]
            let _ = &id;
        }
        Event::UserEvent(UserEvent::Eval(script)) => {
            for entry in state.open.values() {
                if let Err(error) = entry.webview.evaluate_script(&script) {
                    eprintln!("kiln: eval failed: {error}");
                }
            }
        }
        Event::UserEvent(UserEvent::Signal(signal)) => {
            if signal == "devtools" {
                for entry in state.open.values() {
                    open_devtools(&entry.webview);
                }
            }
            state
                .handle
                .emit_to_all("signal", serde_json::json!({ "id": signal }));
        }
        // Mobile lifecycle: tao maps the UIKit callbacks onto
        // WindowEvent::Suspended/Resumed, and the page hears them as
        // `lifecycle` through the same queue Android's activity
        // callbacks use. Desktop never raises either, so this is
        // mobile-only in practice.
        #[cfg(any(target_os = "android", target_os = "ios"))]
        Event::WindowEvent {
            event: WindowEvent::Suspended,
            ..
        } => crate::platform::lifecycle("pause"),
        #[cfg(any(target_os = "android", target_os = "ios"))]
        Event::WindowEvent {
            event: WindowEvent::Resumed,
            ..
        } => crate::platform::lifecycle("resume"),
        _ => {}
    }
}
