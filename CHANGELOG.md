# Changelog

What changed in each release, taken from the commit messages. Consort is
pre-1.0 and its versions say so: anything can move between minor versions,
and the patch number is where most of the work has landed so far.

## 0.8.0 (2026-09-25)

### Added

- **call:** A floating card for the call you are in (@tominal)
- **threads:** Name the panel after the conversation in it (@tominal)
- **shell:** Remember where you were, for back and forward (@tominal)
- **rooms:** Open a room's details from its header (@tominal)
- **rooms:** Close the room's details on Escape (@tominal)
- **timeline:** Delete a message you sent, and mark where it was (@tominal)
- **call:** Put the call card back from the voice strip (@tominal)
- **timeline:** Show joins, leaves, invites, kicks and bans in the room (@bernalalexis-try)
- **timeline:** Draw room name, topic and picture changes (@tominal)
- **matrix:** Know who has read how far, on a channel of its own (@tominal)
- **ui:** Draw who has read a message, and say who cannot be seen (@tominal)
- **app:** Remember which rooms this account has opened (@tominal)
- **ui:** An opening screen with somewhere to go (#83) (@tominal)
- **rooms:** Leave a room, and ask somebody into one (@tominal)
- **app:** Commands for leaving a room and inviting somebody (@tominal)
- **info:** Leave the room, and ask somebody into it (@tominal)
- **rooms:** Read who is in a room, on request rather than in the snapshot (@tominal)
- **info:** A People section under the room's topic (@tominal)
- **ui:** Scale the application, by slider and by Ctrl and plus (@tominal)
- **packaging:** Install one desktop entry, and check the names in it (@tominal)
- **ui:** Put Consort in the system tray (@tominal)
- **emoji:** The standard set, searchable, behind a dynamic import (@tominal)
- **reactions:** The real picker on a message, and a pill for a key with no glyph (@tominal)
- **composer:** The second control, which types the key instead of sending it (@tominal)
- **rooms:** Let somebody walk into a channel a space lists (@tominal)
- **ui:** A space's own screen, with a search over its channels (@tominal)

### Changed

- **call:** Give the sidebar face a component of its own (@tominal)
- **ui:** One clamp for everything that floats (@tominal)
- **ui:** Build the list of readers without indexing past the end (@tominal)
- **matrix:** The store catch-up answers nothing, because nobody asked (@tominal)
- **ui:** One confirmation panel rather than one per thing (@tominal)
- **signed-in:** Drop twelve unmount checks that check nothing (@tominal)
- **ui:** Iterate the zoom listeners directly (@tominal)
- **picker:** Make the remembered keys a category rather than a row (@tominal)

### Documentation

- Say that who has read a message is drawn now (@tominal)
- Say that a room can be left and somebody asked into one (@tominal)
- **coverage:** Say what a covered line does not tell you (@tominal)
- **ui:** Count the text size steps correctly (@tominal)
- **picker:** The cursor is lifted by the handlers, not by itself (@tominal)
- **emoji:** Stop claiming WebKitGTK ignores scrollbar-width (@tominal)
- **coverage:** Take the four numbers from CI rather than from memory (@tominal)
- **audio:** Cut consort-audio's comment down to what the code cannot say (@tominal)

### Fixed

- **ci:** Make the Hygiene gates able to fail (@tominal)
- **ci:** Give the Hygiene container a CA store to clone with (@tominal)
- **ci:** Let the Hygiene steps read the workspace git refuses to trust (@tominal)
- **timeline:** A thread you do not have to aim at (@tominal)
- **shell:** Move one entry per press of the mouse's Back button (@tominal)
- **call:** Put deafen and away behind a chevron (@tominal)
- **call:** Give the words room, and open the panel on a hover too (@tominal)
- **timeline:** Say where a message went when it was sent from a window (@tominal)
- **timeline:** Stay where you were reading, and stop the window lying (@tominal)
- **timeline:** Reaction pills you do not have to aim at (@tominal)
- **shell:** Spend the room a clicked notification asked for (@tominal)
- **shell:** Spend the message a followed link asked for (@tominal)
- **call:** Let the pointer close the panel, and stop the press fighting it (@tominal)
- **timeline:** Give the quoted line above an answer 24px to press (@tominal)
- **call:** Leave the voice channel before the process goes (@tominal)
- **call:** Wait for the leave while the single-instance guard is still up (@tominal)
- **call:** Hide the window from the command, where the hide can take effect (@tominal)
- **timeline:** Keep the way into a thread whose root was deleted (@tominal)
- **receipts:** Put the faces at the end of the line, not the start (@tominal)
- **picker:** Let the control that opened a picker shut it again (@tominal)
- **picker:** Hold the open category by name rather than by position (@tominal)
- **picker:** Lift the arrow cursor as what is under it changes (@tominal)
- **picker:** Three from review, one of them mine (@tominal)
- **emoji:** Take the scrollbar out of the category strip (@tominal)
- **emoji:** Put the category scrollbar back, below the tabs (@tominal)

### New contributors

- @bernalalexis-try made their first contribution

## 0.7.0 (2026-09-22)

### Added

- **shell:** Quit on Ctrl+Q (@tominal)
- **channels:** Count what is unread, not just mark it (@tominal)
- **timeline:** Put a time on every message, not just the first (@tominal)
- **timeline:** Separate the days, and shrink the gutter time (@tominal)
- **timeline:** Edit a message you sent (@tominal)
- **threads:** Put Reply and Edit on messages in a thread (@tominal)

### Fixed

- **ci:** Build the Arch package from the tag's own recipe (@tominal)
- **media:** Address an attachment the way Windows serves it (@tominal)
- **timeline:** Show an edit somebody made, instead of the old text (@tominal)

## 0.6.0 (2026-09-15)

### Added

- **timeline:** Mark what has not been read, and where reading stopped (@tominal)
- **timeline:** Send an attachment (@tominal)
- **notifications:** Say something when Consort is not the window in front (@tominal)

### Fixed

- **timeline:** Line the composer row up (@tominal)
- **timeline:** Read a pasted screenshot in Rust, not in the page (@tominal)
- **deps:** Move vitest past GHSA-82fw-gwwq-j7x9 (@tominal)
- Bump rustls to 0.23.45 for RUSTSEC-2026-0285 (@tominal)

## 0.5.0 (2026-09-05)

### Added

- **timeline:** Shut a thread with Escape (@tominal)
- **channels:** Read a voice channel without connecting to it (@tominal)
- **timeline:** Reply in the room, and follow the links in a message (@tominal)
- **timeline:** Add a reaction from beside the ones already there (@tominal)
- **timeline:** Go to a message that is not loaded (@tominal)

### Fixed

- **viewer:** Scale a tall picture to the window, one level down (@tominal)
- **timeline:** Give an attachment a width its caps can resolve against (@tominal)
- **timeline:** Give the typing line room above the box (@tominal)
- **timeline:** Light the whole row when a jump lands, not the words alone (@tominal)
- **timeline:** Light the name and the picture with the words (@tominal)
- **voice:** Hear the people who stayed when you rejoin a channel (@tominal)

## 0.4.0 (2026-09-03)

### Added

- **timeline:** Make the links in a message links (@tominal)

### Fixed

- **timeline:** Stay at the bottom while a picture is still loading (@tominal)

## 0.3.0 (2026-09-03)

### Added

- **viewer:** Draw the viewer's controls as icons (@tominal)
- **timeline:** Load older messages by scrolling to them (@tominal)
- **timeline:** Start a thread on any message (@tominal)
- **timeline:** React to a message (@tominal)
- **timeline:** Draw custom emoji (@tominal)
- **timeline:** Say who is typing (@tominal)

### Documentation

- Cut the README to the part that is about Consort (@tominal)
- Bring the test counts and the feature list up to date (@tominal)

### Fixed

- **ci:** Install alsa, and enable pnpm the way that works (@tominal)
- **ci:** Run a container job's steps under bash (@tominal)
- **packaging:** Declare libstdc++, which livekit's C++ pulls in (@tominal)
- **viewer:** Scale a tall picture to the window (@tominal)
- **thread:** Open a thread at its newest reply (@tominal)
- **call:** Play somebody whose track stopped and started again (@tominal)

## 0.2.0 (2026-09-03)

### Added

- **timeline:** Say when a message has a thread (@tominal)
- **timeline:** Read a thread (@tominal)
- **timeline:** Keep an open thread up to date (@tominal)
- **timeline:** Open a thread beside the room (@tominal)
- **timeline:** Reply in a thread (@tominal)
- **shell:** Fold the channel list away (@tominal)
- **timeline:** Draw a reply as a reply (@tominal)
- **timeline:** Keep the @, and say when a mention is about you (@tominal)
- **timeline:** Say a thread is opening (@tominal)
- **timeline:** Drag the thread panel wider (@tominal)

### Documentation

- Say that threads are built (@tominal)
- Say that replies and mentions are drawn (@tominal)

### Fixed

- **timeline:** Make a picture's frame the size of the picture again (@tominal)
- **timeline:** Keep a picture's shape in a narrow column (@tominal)

## 0.1.5 (2026-09-02)

### Fixed

- **dialog:** Keep the keyring's zbus off tokio (@tominal)
- **timeline:** Stop a picture growing a pixel at a time (@tominal)
- **timeline:** Take the dead bands off the top and bottom of a room (@tominal)

## 0.1.4 (2026-09-02)

### Added

- **timeline:** Put the room's topic under its name (@tominal)
- **person:** Open a direct message from somebody's card (@tominal)
- **timeline:** Keep the words that came with an attachment (@tominal)
- **media:** Serve attachments over a scheme that can seek (@tominal)
- **timeline:** Save an attachment where you want it (@tominal)
- **timeline:** Show a clip's thumbnail, and say when it cannot be played (@tominal)
- **timeline:** Open a picture full size (@tominal)
- **settings:** Hear what your microphone is sending (@tominal)

### Documentation

- Say what attachments do now, and what codecs they need (@tominal)

### Faster

- **settings:** Stop device enumeration freezing the window (@tominal)

### Fixed

- **shell:** Give the pane back the space empty banners were holding (@tominal)
- **person:** Stop the card claiming everybody is an admin (@tominal)
- **timeline:** Re-read a message once its key arrives (@tominal)

## 0.1.3 (2026-09-02)

### Added

- **voice:** Light the rings from the audio we already handle (@tominal)
- **voice:** Put the gate's thresholds on sliders (@tominal)
- **timeline:** Read and send text in a room (@tominal)
- **timeline:** Open a person's card from their name or their face (@tominal)
- **timeline:** Read and write markdown (@tominal)
- **timeline:** Say whether the person who said something is here (@tominal)
- **timeline:** Draw the pictures and clips a room carries (@tominal)

### Fixed

- **app:** Take the older WebKit renderer when NVIDIA is loaded (@tominal)
- **app:** Say nothing about a session that is verified (@tominal)
- **app:** Keep the session's identifiers out of the room (@tominal)
- **timeline:** Read a message as text rather than as a control (@tominal)
- **timeline:** A message with no key yet is waiting, not broken (@tominal)

## 0.1.1 (2026-09-01)

### Added

- Watch call readiness instead of logging it once at startup (@tominal)
- Refuse a call this session could not be heard in (@tominal)
- An away flag, visible to everybody else in the call (@tominal)
- Play a sound when somebody joins or leaves the voice channel (@tominal)
- Say why a voice channel was not joined (@tominal)
- Say when this session's own audio key never reached the call (@tominal)
- Say out loud what the chimes only announce (@tominal)
- Give the spoken notifications something to say (@tominal)
- Make a call adjustable, and say what it can see (@tominal)
- **voice:** Let one person be turned up to 250% (@tominal)

### Documentation

- Plan the call encryption fix and the presence additions (@tominal)
- Record what the call encryption and presence work turned up (@tominal)
- Record the live results and plan the sounds that are still to come (@tominal)
- Record what the spoken notifications turned up (@tominal)
- Write down what it takes to build on Windows (@tominal)

### Fixed

- Stop a verification flow stranding on a lost state change (@tominal)
- Stop the call notice that could be neither wrong nor dismissed (@tominal)
- Say who is muted, and put a person behind their name (@tominal)
- Give the Windows build a SQLite it can actually link (@tominal)
- Stop the Windows build linking two C runtimes at once (@tominal)
- Join in the only dialect this build can hold a call in (@tominal)
- Hang up with a handset, not a crossed-out speaker (@tominal)
- Keep the sidebar's scrollbar off the person card (@tominal)
- Open the person card beside the sidebar, not over it (@tominal)
- **voice:** Show how long we have been in our own call (@tominal)
- **security:** Encrypt the SDK's state and crypto stores (@tominal)
- **call:** Bound how much of a discovery document is read (@tominal)
- **app:** Name base-uri and form-action in the CSP (@tominal)
- **audio:** Clamp a stored person volume to the ceiling (@tominal)
- **rooms:** Cap the avatar bytes turned into a data URL (@tominal)
- **call:** Stop a forged notice from putting an icon beside somebody (@tominal)

## 0.1.0 (2026-08-28)

### Added

- Matrix authentication in a Tauri desktop shell (@tominal)
- Arch packaging, and drop the AppImage target (@tominal)
- Keyring-backed tokens, and fix sign-in being permanently blocked (@tominal)
- Verify this session by comparing emoji (@tominal)
- Ask another session to verify this one (@tominal)
- Verify this session with a recovery key (@tominal)
- Back up room keys and say whether they are (@tominal)
- Read the account's spaces and channels into a snapshot (@tominal)
- Push the room list to the webview, and fetch avatars on demand (@tominal)
- A three-column shell to hang the room list on (@tominal)
- Draw the rail, the channels, and the avatars (@tominal)
- Name the channels a space lists and nobody here joined (@tominal)
- Show who is in a voice channel without joining it (@tominal)
- The voice gate and the device catalogue (@tominal)
- Audio settings that survive a restart (@tominal)
- The thread that owns the microphone (@tominal)
- The microphone test, reaching the webview (@tominal)
- Settings, behind a gear, with a level meter that moves (@tominal)
- A test tone, so the output picker has something to show for itself (@tominal)
- Voice activity detection as a switch, not a policy (@tominal)
- A thread that can be in a voice call, and a gate on whether it is worth joining (@tominal)
- Carry the microphone into a call (@tominal)
- Join and leave a voice channel from the interface (@tominal)
- Draw the call's own roster, and say why a call cannot be heard (@tominal)
- **voice:** Hear the call, and show who is speaking or deafened (@tominal)

### Documentation

- Turn the verification plan from a draft into one that can be built (@tominal)
- Make the dev build command carry the GBM workaround (@tominal)
- The room list row is working now (@tominal)
- Correct the account of what happens to a busy device (@tominal)
- Plan the connect half of the voice channel (@tominal)
- Record what phase 0 actually found (@tominal)

### Fixed

- Stop the AUR package leaking the builder's home directory (@tominal)
- Keep the user panel in shape, and let the shell hot reload (@tominal)
- List only the audio devices we can actually open (@tominal)
- Escape, and a close button that fits inside its own ring (@tominal)
- Make voice calls work against a pre-MSC4354 deployment (@tominal)

### New contributors

- @tominal made their first contribution

