//! Test helper: owns a "default" metadata object like WirePlumber does. Killing it simulates a
//! WirePlumber restart. Only for use against a private PipeWire instance.
use pipewire as pw;

fn main() -> Result<(), pw::Error> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let _md: pw::metadata::Metadata =
        core.create_object("metadata", &pw::properties::properties! { "metadata.name" => "default" })?;
    mainloop.run();
    Ok(())
}
