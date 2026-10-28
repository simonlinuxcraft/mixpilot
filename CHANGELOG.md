# Changelog

## Unreleased

- Auto level for the microphone no longer creeps up during pauses, so the first word after a pause does not come in too loud. A shout or a laugh is turned down within 10 ms instead of being flattened by the limiter, and the voice after a laugh stays within about 2 dB instead of dropping by 5 dB or more. Single clicks and knocks that get past the gate no longer make it turn up. Tuned on a real recording, quiet speech gets the same lift as before
- The microphone page warns when the microphone already clips before Mixpilot gets the sound, which no processing can repair, and says to turn its level down in the system's sound settings
- Two new voice presets. Podcast: the full, close-miked podcast voice with dense compression, clearer than Warm and with less boom and edge than Broadcast. Laptop mic: for microphones built into laptops and webcams, with less room sound and hum and the speech band brought forward
- Warm rolls off the top more gently, from 8 kHz instead of 6 kHz, so it sounds soft instead of muffled. Broadcast lifts 120 Hz by 5 dB instead of 7, so deep voices no longer boom
- A muted microphone is silent from the very first moment. After a restart of the audio core, or when you muted while no app was listening, the next app that opened the microphone still got about 80 ms of your voice
- Setting the input back to "System default" after picking a fixed microphone works again. Mixpilot kept recording the old device until it was restarted

## 0.0.6 - Easier for everyone

- Keyboard and screen readers: the tabs switch with the arrow keys, dialogs take the focus and give it back, every mute button names its channel, sliders and the microphone levels read out their values in words, and errors and the audio core status are announced. Ctrl+Plus and Ctrl+Minus zoom both windows, the content scrolls only when zoomed in. Slider tracks, unlit meter segments and the rules dimmed while Autopilot is off have more contrast
- Three new built-in presets. Music: full deep bass without boom, a little less box around 1 kHz and some air on top; the Music channel starts with it. Movie: dialogue a little clearer, some weight for effects. Laptop: no deep bass that small speakers only distort, more fullness instead. An own preset with one of these names becomes the built-in one, its curve kept as a tuning that Reset takes back
- Push-to-mute connects again by itself when the desktop's shortcut service is not ready yet at login or restarts later, and only the desktop itself can trigger the key
- No more gaps in the channels and when hearing yourself while PipeWire runs with a quantum of 4096 or 8192
- A hung audio core is replaced even while the window is closed
- Bug reports arrive with their line breaks and contain exactly the system info and log the preview showed
- Esc in a value field keeps the value, and tabbing through the fields no longer nudges the volumes
- Switching night mode on or off in the window shows up in the Autopilot log
- Less work in the background: with the window closed the audio core updates its state once a second instead of 20 times, and the master volume is read only after it changed
- Without XDG_RUNTIME_DIR, Mixpilot no longer falls back to a shared folder in /tmp that another user on the same computer could take over

## 0.0.5 - Steadier every day

- A damaged settings file no longer leaves Mixpilot stuck on the welcome screen with the tray and the mute key doing nothing. The core keeps the file as config.json.broken-<date> next to it, loads the last working settings or the defaults, and the window says so once. A device name with a NUL character in the settings no longer crashes the core
- A hanging PulseAudio service (pipewire-pulse) no longer freezes Mixpilot. Volume reads give up after 2 seconds instead of 30 and never pile up, so the device list, microphone test and push-to-mute keep working, and "Report a bug" opens right away
- Starting Mixpilot twice at nearly the same time, for example by autostart and a click at login, no longer runs two copies with two tray icons, where quitting one let the other start the audio core again

## 0.0.4 - Stability fixes

- The audio core comes back by itself when PipeWire restarts or the core crashes, so channels and the Mixpilot microphone return within seconds instead of staying gone until Mixpilot is restarted. The core log keeps earlier runs, so the cause of a crash is still in a bug report
- No more rare dropouts and clicks when monitoring the microphone or recording from the Mixpilot microphone. When the microphone and the headset are separate devices, their clocks run a little apart; Mixpilot now follows that drift instead of letting a buffer run dry or overflow every few minutes
- Opening the window while the computer is busy no longer restarts a working audio core, which cut the sound for a moment. Only an older or hung core is replaced
- The microphone no longer loses pieces of audio when PipeWire runs with a quantum above 2048, as some setups do to save power

## 0.0.3 - Music and channel EQ

- Music channel: music players like Spotify, Rhythmbox or Amberol get their own channel next to Media, so music can have its own volume and sound. Video players and browsers stay in Media. Existing setups move the music players over on the first start, your own assignments stay as they are
- An equalizer per channel: the EQ button of each mixer strip picks a preset. The sound tab edits presets, built-in ones can be tuned and reset, a new one starts as a copy of the one on screen. The single equalizer for everything is gone, a curve tuned there is kept as the preset "My EQ"
- Mute and EQ buttons of a mixer strip sit on top of each other, so preset names fit with five channels

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
