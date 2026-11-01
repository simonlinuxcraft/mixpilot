# Changelog

## Unreleased

- Switching Autopilot off in the tray menu sticks, even while a fader in the window is being moved. Before, the window could switch it back on a moment later
- When sending a bug report fails on a server error page, the window names the HTTP status instead of an empty reason
- The .deb package names what Mixpilot needs (PipeWire library and tools, pactl, pgrep, curl, xdg-open), so apt installs it along instead of the audio core failing to start
- A microphone that keeps failing is retried less and less often, up to once a minute, instead of every 2 seconds. Before, the window showed "Microphone active" over and over and the log grew by about 1800 lines an hour
- No more click on every channel when an app with low latency starts, for example a call or a game. Cutting the buffer down to the shorter cycle now blends like the other cuts
- The microphone page says when no sound at all has come from the microphone for 10 seconds, for example when the system default is an empty jack, and names the device
- Bug reports from the Snap name the real distribution instead of "Ubuntu Core", say which Snap permissions are not connected, and list the default devices, all outputs and microphones and the current microphone level
- Auto level for the microphone no longer creeps up during pauses, so the first word after a pause does not come in too loud. A shout or a laugh is turned down within 10 ms instead of being flattened by the limiter, and the voice after a laugh stays within about 2 dB instead of dropping by 5 dB or more. Single clicks and knocks that get past the gate no longer make it turn up. Tuned on a real recording, quiet speech gets the same lift as before
- The microphone page warns when the microphone already clips before Mixpilot gets the sound, which no processing can repair, and says to turn its level down in the system's sound settings
- Two new voice presets. Podcast: the full, close-miked podcast voice with dense compression, clearer than Warm and with less boom and edge than Broadcast. Laptop mic: for microphones built into laptops and webcams, with less room sound and hum and the speech band brought forward
- Warm rolls off the top more gently, from 8 kHz instead of 6 kHz, so it sounds soft instead of muffled. Broadcast lifts 120 Hz by 5 dB instead of 7, so deep voices no longer boom
- A muted microphone is silent from the very first moment. After a restart of the audio core, or when you muted while no app was listening, the next app that opened the microphone still got about 80 ms of your voice
- Setting the input back to "System default" after picking a fixed microphone works again. Mixpilot kept recording the old device until it was restarted
- Without any microphone, for example when the only headset is unplugged during a call, the Mixpilot microphone goes quiet and the window says "No microphone". Before, the system handed it the sound output instead, so the others heard the game and their own voices coming back
- Changing the input device or a short dropout of a USB or Bluetooth microphone no longer adds 10 ms of delay each time to the Mixpilot microphone and to hearing yourself
- If unmuting fails because the settings file cannot be written, the window says so instead of showing the microphone as active
- Bass, Clarity and the equalizer no longer overdrive the output. A loud song with full bass reached the limiter with up to 17 dB too much and was audibly distorted, however far the system volume was turned down. Each channel now gives up exactly as much level as its boosts add, so the sound stays clean. With strong boosts the channels play quieter than before, turn the channel or the system volume up to match
- The limiter no longer distorts bass. It let the level rise again between two peaks of a bass wave, which roughened every loud song that reached it. Now it holds still through the whole wave, the distortion on a 30 to 60 Hz bass drops from up to 2 % to well below what anyone can hear
- Ducking reacts in time. Game, Media and Music are down by 10 dB within 100 ms of someone talking in Chat instead of after almost a second, so a short call-out is heard, and are back to full level about a second after the last word instead of up to two. It also works when the Chat fader is low, which used to stop it completely
- Sound no longer stays late after switching the output device or a short stall. Up to three audio cycles of delay could remain for good, about 64 ms at the usual settings and a quarter second at large buffer sizes; they are now dropped within a few seconds. When an app's audio runs dry for a moment it fades out and back in instead of clicking
- Switching sound settings no longer jumps. Changing or turning off auto volume, also when night mode starts or ends by itself, used to change the level by 6 to 8 dB at once and now glides. Night mode fades the bass out over about half a second. An equalizer band set back to 0 dB no longer drops out mid-wave, and turning Clarity or the limiter back on no longer plays a moment of old sound
- Microphone: a new session no longer starts with a scrap of your voice from the last one, also when hearing yourself, and switching the input device while it is open starts the auto level and the gate fresh for the new microphone. Changing the voice preset or the S sound setting mid-sentence no longer clicks, and turning noise suppression on or off fades over 10 ms instead of jumping. When the last microphone goes away the window says so instead of "nobody is using it"
- Less CPU load during long silences with auto volume, the gate or the S sound control on
- Error messages can be closed, and they go away by themselves once their cause is gone, for example after unmuting the microphone or when a second running Mixpilot was closed. Before, they stayed until some other setting changed
- Naming a new equalizer preset after one of your own presets no longer overwrites that preset without asking, the name is refused like the built-in names. Night mode cannot be set to start and end at the same hour anymore, which made it never start. The microphone status reads the current device list, so "No microphone" shows up at once instead of up to 5 s later
- Master keeps a left/right balance set in the system instead of evening it out with the first move, and follows the volume keys even right after you clicked it. Above 100 % its range reaches 150 %, so touching it no longer cuts a 130 % volume down to 100 %
- Quitting no longer freezes the window and the tray for a few seconds while the audio core stops. When the tray cannot save a change, for example because the settings file is not writable, its check mark goes back instead of showing a state that is not in effect. A bug report keeps the core log even when it holds a few bytes that are not valid text
- Push-to-mute shows as on again after its key was removed and set again in the desktop's settings. It kept working but the window said it was off

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
