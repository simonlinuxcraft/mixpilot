# Changelog

## Unreleased

- Music channel: music players like Spotify, Rhythmbox or Amberol get their own channel next to Media, so music can have its own volume and sound. Video players and browsers stay in Media. Existing setups move the music players over on the first start, your own assignments stay as they are
- An equalizer per channel: the EQ button of each mixer strip picks a preset. The sound tab edits presets, built-in ones can be tuned and reset, a new one starts as a copy of the one on screen. The single equalizer for everything is gone, a curve tuned there is kept as the preset "My EQ"

## 0.0.2 - Push-to-mute and presets

- Push-to-mute: a global key mutes and unmutes the microphone. Switch it on in the microphone tab, the desktop asks for the key in its own dialog (GNOME 48 or newer, KDE Plasma)
- Save up to four EQ presets of your own
- Microphone test: record five seconds of the processed voice and hear them back
- Broadcast voice preset sounds like radio now: about 10 dB of fast compression and stronger proximity bass
- S sounds: a de-esser for every voice preset, off, normal or strong, in the noise suppression card
- The equalizer shows its curve as LED columns
- The mixer strips fill the window and the faders grow with it. The window opens at its minimum size
- A what's new dialog after an update, also reachable from the about dialog
- The instant mute switch left the microphone tab, it stays in the tray menu
- The device lists come straight from the audio core, so the occasional "Could not read the device list" error is gone on every PipeWire version

## 0.0.1 - First release

- Game, Chat, Media and Aux channels with ChatMix, ducking and automatic sorting
- Equalizer, bass boost, auto volume and limiter
- Virtual microphone with noise suppression, gate, auto level and voice presets
- Tray icon, autostart, English and German interface
