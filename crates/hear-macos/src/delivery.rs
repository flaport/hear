use anyhow::{Context, Result};
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

pub fn deliver(transcript: &str, paste: bool) -> Result<bool> {
    arboard::Clipboard::new()
        .context("could not access the clipboard")?
        .set_text(transcript)
        .context("could not copy the transcript")?;

    if paste && accessibility_is_trusted() {
        post_paste()?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn accessibility_is_trusted() -> bool {
    unsafe { objc2_application_services::AXIsProcessTrusted() }
}

fn post_paste() -> Result<()> {
    const V_KEY_CODE: u16 = 9;
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("could not create a keyboard event source"))?;
    let key_down = CGEvent::new_keyboard_event(source.clone(), V_KEY_CODE, true)
        .map_err(|_| anyhow::anyhow!("could not create the paste key-down event"))?;
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);
    let key_up = CGEvent::new_keyboard_event(source, V_KEY_CODE, false)
        .map_err(|_| anyhow::anyhow!("could not create the paste key-up event"))?;
    key_up.set_flags(CGEventFlags::CGEventFlagCommand);
    key_down.post(CGEventTapLocation::HID);
    key_up.post(CGEventTapLocation::HID);
    Ok(())
}
