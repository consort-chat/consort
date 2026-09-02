# Changelog

What changed in each release, taken from the commit messages. Consort is
pre-1.0 and its versions say so: anything can move between minor versions,
and the patch number is where most of the work has landed so far.

## 0.1.4 (2026-09-02)

### Added

- **timeline:** Put the room's topic under its name
- **person:** Open a direct message from somebody's card
- **timeline:** Keep the words that came with an attachment
- **media:** Serve attachments over a scheme that can seek
- **timeline:** Save an attachment where you want it
- **timeline:** Show a clip's thumbnail, and say when it cannot be played
- **timeline:** Open a picture full size
- **settings:** Hear what your microphone is sending

### Documentation

- Say what attachments do now, and what codecs they need

### Faster

- **settings:** Stop device enumeration freezing the window

### Fixed

- **shell:** Give the pane back the space empty banners were holding
- **person:** Stop the card claiming everybody is an admin
- **timeline:** Re-read a message once its key arrives

## 0.1.3 (2026-09-02)

### Added

- **voice:** Light the rings from the audio we already handle
- **voice:** Put the gate's thresholds on sliders
- **timeline:** Read and send text in a room
- **timeline:** Open a person's card from their name or their face
- **timeline:** Read and write markdown
- **timeline:** Say whether the person who said something is here
- **timeline:** Draw the pictures and clips a room carries

### Fixed

- **app:** Take the older WebKit renderer when NVIDIA is loaded
- **app:** Say nothing about a session that is verified
- **app:** Keep the session's identifiers out of the room
- **timeline:** Read a message as text rather than as a control
- **timeline:** A message with no key yet is waiting, not broken

## 0.1.1 (2026-09-01)

### Added

- Watch call readiness instead of logging it once at startup
- Refuse a call this session could not be heard in
- An away flag, visible to everybody else in the call
- Play a sound when somebody joins or leaves the voice channel
- Say why a voice channel was not joined
- Say when this session's own audio key never reached the call
- Say out loud what the chimes only announce
- Give the spoken notifications something to say
- Make a call adjustable, and say what it can see
- **voice:** Let one person be turned up to 250%

### Documentation

- Plan the call encryption fix and the presence additions
- Record what the call encryption and presence work turned up
- Record the live results and plan the sounds that are still to come
- Record what the spoken notifications turned up
- Write down what it takes to build on Windows

### Fixed

- Stop a verification flow stranding on a lost state change
- Stop the call notice that could be neither wrong nor dismissed
- Say who is muted, and put a person behind their name
- Give the Windows build a SQLite it can actually link
- Stop the Windows build linking two C runtimes at once
- Join in the only dialect this build can hold a call in
- Hang up with a handset, not a crossed-out speaker
- Keep the sidebar's scrollbar off the person card
- Open the person card beside the sidebar, not over it
- **voice:** Show how long we have been in our own call
- **security:** Encrypt the SDK's state and crypto stores
- **call:** Bound how much of a discovery document is read
- **app:** Name base-uri and form-action in the CSP
- **audio:** Clamp a stored person volume to the ceiling
- **rooms:** Cap the avatar bytes turned into a data URL
- **call:** Stop a forged notice from putting an icon beside somebody

## 0.1.0 (2026-08-28)

### Added

- Matrix authentication in a Tauri desktop shell
- Arch packaging, and drop the AppImage target
- Keyring-backed tokens, and fix sign-in being permanently blocked
- Verify this session by comparing emoji
- Ask another session to verify this one
- Verify this session with a recovery key
- Back up room keys and say whether they are
- Read the account's spaces and channels into a snapshot
- Push the room list to the webview, and fetch avatars on demand
- A three-column shell to hang the room list on
- Draw the rail, the channels, and the avatars
- Name the channels a space lists and nobody here joined
- Show who is in a voice channel without joining it
- The voice gate and the device catalogue
- Audio settings that survive a restart
- The thread that owns the microphone
- The microphone test, reaching the webview
- Settings, behind a gear, with a level meter that moves
- A test tone, so the output picker has something to show for itself
- Voice activity detection as a switch, not a policy
- A thread that can be in a voice call, and a gate on whether it is worth joining
- Carry the microphone into a call
- Join and leave a voice channel from the interface
- Draw the call's own roster, and say why a call cannot be heard
- **voice:** Hear the call, and show who is speaking or deafened

### Documentation

- Turn the verification plan from a draft into one that can be built
- Make the dev build command carry the GBM workaround
- The room list row is working now
- Correct the account of what happens to a busy device
- Plan the connect half of the voice channel
- Record what phase 0 actually found

### Fixed

- Stop the AUR package leaking the builder's home directory
- Keep the user panel in shape, and let the shell hot reload
- List only the audio devices we can actually open
- Escape, and a close button that fits inside its own ring
- Make voice calls work against a pre-MSC4354 deployment

