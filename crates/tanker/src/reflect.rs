//! Reflections of characters in the stages' mirrors (`mirror.c`, `chara
//! #0d02fd <name> -p x y z -a axis`, `FUN_0039e440`): axis 1 mirrors in
//! the floor plane at height y (the decks' wet floors), 2 in the wall plane
//! at z (w01's washroom). The scenery's own reflections are built into the
//! levels; the actor draws the characters it is told to, flipped across
//! the plane (`FUN_0039db88`): message 0 <actor> starts reflecting that
//! actor, 1 stops (`FUN_0039d6a0` / `FUN_0039d860`; the scripts switch
//! Snake and the soldiers on and off with traps round the reflecting
//! areas). Here the reflection is the character's skinned meshes drawn a
//! second time through the mirror matrix, at reduced strength (assumed).

use bevy::prelude::*;
use mgs2_bevy::scene::SCALE;

use crate::body::Body;

pub const MIRROR_ACTOR: u32 = 0x0D02FD;

/// A mirror: its plane and the actors it currently reflects.
#[derive(Clone, Debug)]
pub struct Mirror {
    pub name: u32,
    /// 1: the plane y = at; 2: the plane z = at (mm).
    pub axis: i32,
    pub at: f32,
    pub actors: Vec<u32>,
}

impl Mirror {
    pub fn from_spawn(s: &crate::gcl::Spawn) -> Option<Self> {
        let name = s.args.first().and_then(|v| v.int())? as u32 & 0x00FF_FFFF;
        let axis = s.opt_int(b'a', 0).unwrap_or(0);
        let at = match axis {
            1 => s.opt_int(b'p', 1)?,
            2 => s.opt_int(b'p', 2)?,
            _ => return None,
        } as f32;
        Some(Self { name, axis, at, actors: Vec::new() })
    }

    /// The mirror's matrix in world space (metres).
    fn matrix(&self) -> Mat4 {
        let at = self.at * SCALE;
        match self.axis {
            1 => Mat4::from_translation(Vec3::Y * 2.0 * at) * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0)),
            _ => Mat4::from_translation(Vec3::Z * 2.0 * at) * Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0)),
        }
    }
}

/// Which actor a body is, for the mirrors (Snake or a soldier by name).
#[derive(Component, Clone, Copy)]
pub struct ActorName(pub u32);

/// A reflected copy of one of a body's meshes.
#[derive(Component)]
struct Reflection {
    source: Entity,
    mirror: u32,
}

pub fn plugin(app: &mut App) {
    app.add_systems(PostUpdate, update.after(bevy::transform::TransformSystems::Propagate));
}

/// A mirror message: 0 reflect `actor`, 1 stop.
pub fn message(mirrors: &mut [Mirror], name: u32, code: u32, actor: u32) -> bool {
    let Some(m) = mirrors.iter_mut().find(|m| m.name == name) else { return false };
    let actor = actor & 0x00FF_FFFF;
    match code {
        0 if !m.actors.contains(&actor) => {
            info!("mirror {name:06x} reflects {actor:06x}");
            m.actors.push(actor)
        }
        1 => m.actors.retain(|a| *a != actor),
        _ => {}
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn update(
    mut commands: Commands,
    game: Res<crate::Game>,
    bodies: Query<(Entity, &ActorName), With<Body>>,
    children: Query<&Children>,
    parts: Query<(&Mesh3d, &MeshMaterial3d<StandardMaterial>, &GlobalTransform, &InheritedVisibility), Without<Reflection>>,
    mut copies: Query<(Entity, &Reflection, &mut Transform, &mut Visibility)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut faded: Local<std::collections::HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>>,
) {
    // The (mirror, part) pairs wanted now.
    let mut wanted = Vec::new();
    for m in &game.mirrors {
        for (body, name) in &bodies {
            if !m.actors.contains(&name.0) {
                continue;
            }
            for c in children.iter_descendants(body) {
                if parts.contains(c) {
                    wanted.push((m.name, c, m.matrix()));
                }
            }
        }
    }
    // Update or drop the existing copies.
    let mut have = Vec::new();
    for (e, r, mut t, mut v) in &mut copies {
        let Some((_, _, mat)) = wanted.iter().find(|w| w.0 == r.mirror && w.1 == r.source) else {
            commands.entity(e).despawn();
            continue;
        };
        let Ok((_, _, g, shown)) = parts.get(r.source) else {
            commands.entity(e).despawn();
            continue;
        };
        *t = Transform::from_matrix(*mat * g.to_matrix());
        let want = if shown.get() { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
        have.push((r.mirror, r.source));
    }
    // New copies.
    for (mirror, part, mat) in wanted {
        if have.contains(&(mirror, part)) {
            continue;
        }
        let Ok((mesh, material, g, _)) = parts.get(part) else { continue };
        let fade = faded
            .entry(material.0.id())
            .or_insert_with(|| {
                let mut m = materials.get(&material.0).cloned().unwrap_or_default();
                // Seen through the floor's own surface: fainter, both faces
                // (the mirror flips the winding).
                m.cull_mode = None;
                m.base_color = m.base_color.with_alpha(0.45);
                m.alpha_mode = AlphaMode::Blend;
                materials.add(m)
            })
            .clone();
        commands.spawn((
            Reflection { source: part, mirror },
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(fade),
            Transform::from_matrix(mat * g.to_matrix()),
            Visibility::default(),
            crate::LevelEntity,
        ));
    }
}
