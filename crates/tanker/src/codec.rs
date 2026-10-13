//! Codec calls from CODEC.DAT.
//!
//! CODEC.DAT is a run of GCL script records (see `re/notes/formats.md`).
//! Stage scripts name a record by id: its low 24 bits times 16 is the
//! record's offset. A record's `main` picks the conversation from the story
//! variables; its `#3b91eb <speaker> .. -m str[n]` commands are the lines.
//!
//! K calls the frequency the area has set (141.12, Otacon); incoming calls
//! (`command #4a243a -c`) open by themselves. Space/Enter next line, Esc
//! hang up. Play pauses while the Codec is open.

use std::io::Read;

use bevy::prelude::*;
use mgs2_formats::gcx::{self, Gcx};

use crate::Game;
use crate::gcl::{Banks, Vm};

/// Otacon's frequency (141.12) and his save frequency (140.96).
pub const OTACON_FREQ: i32 = 14112;
pub const SAVE_FREQ: i32 = 14096;

#[derive(Resource)]
pub struct CodecData {
    data: Vec<u8>,
    pub iso: std::path::PathBuf,
}

impl CodecData {
    pub fn load(iso: &std::path::Path) -> anyhow::Result<Self> {
        let mut data = Vec::new();
        mgs2_formats::iso::Iso::open(iso)?.open_file("MGS2/CODEC.DAT")?.read_to_end(&mut data)?;
        Ok(Self { data, iso: iso.to_path_buf() })
    }

    /// A voice clip (`0x20000000 + sector` in VOX.DAT) as a WAV file.
    pub fn voice(&self, id: u32) -> anyhow::Result<Vec<u8>> {
        Ok(self.clip(id)?.0.wav())
    }

    /// A spoken clip and its subtitles. `0x20000000 + sector` names a clip
    /// in VOX.DAT, or, where none starts there, in MOVIE.DAT (the holds'
    /// speech: the same chunks plus an IPU video stream).
    pub fn clip(&self, id: u32) -> anyhow::Result<(mgs2_formats::vox::Clip, Vec<mgs2_formats::demo::Line>)> {
        let bytes = self.clip_chunks(id)?;
        let clip = mgs2_formats::vox::decode_clip(&bytes).ok_or_else(|| anyhow::anyhow!("no audio in voice {id:#x}"))?;
        Ok((clip, mgs2_formats::demo::parse(&bytes).subtitles))
    }

    /// A spoken clip's raw chunks (VOX.DAT, else MOVIE.DAT).
    pub fn clip_chunks(&self, id: u32) -> anyhow::Result<Vec<u8>> {
        let sector = u64::from(id.wrapping_sub(0x2000_0000));
        let bytes = Self::chunks(&self.iso, "MGS2/VOX.DAT", sector)?;
        if !bytes.is_empty() {
            return Ok(bytes);
        }
        Self::chunks(&self.iso, "MGS2/MOVIE.DAT", sector)
    }

    /// The chunks of the clip starting at `sector` of `file` (none if no
    /// clip header is there).
    fn chunks(iso: &std::path::Path, file: &str, sector: u64) -> anyhow::Result<Vec<u8>> {
        use std::io::{Seek, SeekFrom};
        let mut f = mgs2_formats::iso::Iso::open(iso)?.open_file(file)?;
        let mut pos = sector * 0x800;
        let mut bytes = Vec::new();
        let mut head = [0u8; 8];
        f.seek(SeekFrom::Start(pos))?;
        f.read_exact(&mut head)?;
        if head != [0x10, 0, 0, 0, 0x10, 0, 0, 0] {
            return Ok(bytes);
        }
        let mut seen = false;
        for _ in 0..20_000 {
            f.seek(SeekFrom::Start(pos))?;
            f.read_exact(&mut head)?;
            let kind = u32::from_le_bytes(head[..4].try_into().unwrap());
            let size = u32::from_le_bytes(head[4..].try_into().unwrap()) as u64;
            if !(16..=0x10_0000).contains(&size) || (kind == 0x10 && seen) {
                break;
            }
            seen |= kind != 0x10;
            let mut chunk = vec![0u8; size as usize];
            f.seek(SeekFrom::Start(pos))?;
            f.read_exact(&mut chunk)?;
            bytes.extend_from_slice(&chunk);
            pos += size;
            if kind == 0xF0 {
                break;
            }
        }
        Ok(bytes)
    }

    /// The record a stage script's codec id points at.
    pub fn record(&self, id: u32) -> Option<Gcx> {
        let start = (id & 0x00FF_FFFF) as usize * 16;
        let stamp = self.data.get(..4)?;
        if self.data.get(start..start + 4)? != stamp {
            return None;
        }
        let end = (start + 16..self.data.len().saturating_sub(4))
            .step_by(16)
            .find(|&i| &self.data[i..i + 4] == stamp)
            .unwrap_or(self.data.len());
        gcx::parse(&self.data[start..end]).ok()
    }
}

/// Runs a record's `main` with the current variables and returns its lines
/// and the variables afterwards (calls set "heard" flags).
pub fn play(record: &Gcx, banks: Banks) -> (Vec<(u32, String)>, Vec<u32>, Banks) {
    let mut vm = Vm::with_banks(record, banks);
    vm.codec_mode = true;
    vm.run(&record.main);
    let lines = std::mem::take(&mut vm.lines);
    let voices = std::mem::take(&mut vm.line_voices);
    let (_, _, _, banks) = vm.finish();
    (lines, voices, banks)
}

pub fn speaker(h: u32) -> &'static str {
    match h {
        0x4AB569 => "OTACON",
        0x28DCE6 => "SNAKE",
        _ => "???",
    }
}

/// An open call.
pub struct Call {
    pub freq: i32,
    pub lines: Vec<(u32, String)>,
    /// Voice clip of each line (0 = none).
    pub voices: Vec<u32>,
    pub index: usize,
    /// Script proc to run when the call ends.
    pub then: Option<u32>,
}

#[derive(Resource, Default)]
pub struct CodecState {
    pub call: Option<Call>,
}

pub fn idle(state: Res<CodecState>) -> bool {
    state.call.is_none()
}

#[derive(Component)]
pub struct CodecScreen;

pub fn setup_screen(mut commands: Commands) {
    commands.spawn((
        CodecScreen,
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(22.0), ..default() },
        TextColor(Color::srgb(0.55, 1.0, 0.65)),
        BackgroundColor(Color::srgba(0.0, 0.05, 0.02, 0.88)),
        Node {
            position_type: PositionType::Absolute,
            left: percent(10),
            right: percent(10),
            bottom: percent(8),
            padding: UiRect::all(px(18)),
            ..default()
        },
        Visibility::Hidden,
    ));
}

fn open(game: &mut Game, data: &CodecData, freq: i32, id: u32, then: Option<u32>) -> Option<Call> {
    let Some(record) = data.record(id) else {
        game.codec_then.extend(then);
        return None;
    };
    let banks = std::mem::replace(&mut game.banks, Banks { a: Vec::new(), b: Vec::new() });
    let (lines, voices, banks) = play(&record, banks);
    game.banks = banks;
    for (who, l) in &lines {
        info!("codec {}: {}", speaker(*who), l.replace('\n', " "));
    }
    if lines.is_empty() {
        game.codec_then.extend(then);
        return None;
    }
    Some(Call { freq, lines, voices, index: 0, then })
}

/// `--codec-at`: press K at this time (seconds) for automated tests; the
/// flag also pages through every call by itself (`--codec-auto`).
#[derive(Resource)]
pub struct AutoCall(pub f32, pub bool);

pub fn input(
    time: Res<Time<Real>>,
    auto: Option<ResMut<AutoCall>>,
    keys: Res<ButtonInput<KeyCode>>,
    data: Option<Res<CodecData>>,
    mut state: ResMut<CodecState>,
    mut game: ResMut<Game>,
) {
    let Some(data) = data else { return };
    if let Some(call) = state.call.as_mut() {
        let auto_next = auto.as_ref().is_some_and(|a| a.1);
        let end = if keys.just_pressed(KeyCode::Escape) {
            true
        } else if keys.just_pressed(KeyCode::Space) || keys.just_pressed(KeyCode::Enter) || auto_next {
            call.index += 1;
            call.index >= call.lines.len()
        } else {
            false
        };
        if end {
            game.codec_then.extend(call.then);
            // Otacon's save frequency: the game saves when the call ends.
            if call.freq == SAVE_FREQ {
                game.save_requested = true;
            }
            state.call = None;
        }
        return;
    }
    if let Some((freq, id, then)) = game.codec_calls.pop() {
        state.call = open(&mut game, &data, freq, id, then);
    } else if keys.just_pressed(KeyCode::KeyK) || auto.is_some_and(|mut a| a.0 <= time.elapsed_secs() && { a.0 = f32::MAX; true }) {
        let base = game.codec_base.iter().find(|b| b.0 == OTACON_FREQ).copied();
        let call = match game.codec_area.filter(|a| a.0 == OTACON_FREQ).or(base) {
            Some((freq, id)) => open(&mut game, &data, freq, id, None),
            None => None,
        };
        state.call = Some(call.unwrap_or(Call { freq: OTACON_FREQ, lines: vec![(0, "(no answer)".into())], voices: vec![0], index: 0, then: None }));
    } else if keys.just_pressed(KeyCode::KeyL) {
        // L: 140.96, where Otacon saves the game.
        let base = game.codec_base.iter().find(|b| b.0 == SAVE_FREQ).copied();
        let call = base.and_then(|(freq, id)| open(&mut game, &data, freq, id, None));
        state.call = Some(call.unwrap_or(Call { freq: SAVE_FREQ, lines: vec![(0, "(no answer)".into())], voices: vec![0], index: 0, then: None }));
    }
}

/// The voice clip playing in the Codec.
#[derive(Component)]
pub struct CodecVoice;

/// Plays the voice clip of the line on screen (each clip covers a run of
/// lines) and stops it when the call ends.
pub fn voice(
    mut commands: Commands,
    state: Res<CodecState>,
    data: Option<Res<CodecData>>,
    mut sources: ResMut<Assets<AudioSource>>,
    playing: Query<Entity, With<CodecVoice>>,
    mut started: Local<Option<u32>>,
) {
    let stop = |commands: &mut Commands| {
        for e in &playing {
            commands.entity(e).despawn();
        }
    };
    if state.call.is_none() {
        if started.take().is_some() {
            stop(&mut commands);
        }
        return;
    }
    // A clip covers a run of lines: it starts with the first of them and
    // plays on (or ends by itself) while the others are shown.
    let want = state.call.as_ref().and_then(|c| c.voices.get(c.index).copied()).filter(|v| *v != 0);
    if want.is_none() || want == *started {
        return;
    }
    stop(&mut commands);
    *started = want;
    let (Some(id), Some(data)) = (want, data) else { return };
    match data.voice(id) {
        Ok(wav) => {
            let secs = (wav.len() - 44) as f32 / u32::from_le_bytes([wav[28], wav[29], wav[30], wav[31]]) as f32;
            info!("codec voice {id:#x}: {secs:.1} s");
            let handle = sources.add(AudioSource { bytes: wav.into() });
            commands.spawn((AudioPlayer(handle), PlaybackSettings::DESPAWN, CodecVoice));
        }
        Err(e) => warn!("codec voice {id:#x}: {e:#}"),
    }
}

pub fn screen(state: Res<CodecState>, mut q: Query<(&mut Text, &mut Visibility), With<CodecScreen>>) {
    let Ok((mut t, mut v)) = q.single_mut() else { return };
    match &state.call {
        Some(call) => {
            *v = Visibility::Inherited;
            let (who, line) = &call.lines[call.index.min(call.lines.len() - 1)];
            **t = format!(
                "CODEC  {}.{:02}   [{}/{}]\n\n{}:\n{}\n\nSpace: next   Esc: end",
                call.freq / 100,
                call.freq % 100,
                call.index + 1,
                call.lines.len(),
                speaker(*who),
                line.trim_end()
            );
        }
        None => *v = Visibility::Hidden,
    }
}
