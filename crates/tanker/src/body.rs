//! Skinned, animated characters (Snake, guards).

use bevy::prelude::*;
use mgs2_bevy::anim;
use mgs2_bevy::scene::{Part, SCALE};

use crate::Game;

/// Placement and animation state of a character. Units are mm.
#[derive(Component)]
pub struct Body {
    /// Feet position.
    pub pos: Vec3,
    /// Facing; the models face +Z at 0.
    pub yaw: f32,
    /// Index into `StageData::models`.
    pub model: usize,
    /// Index into `StageData::motions`.
    pub motions: Option<usize>,
    pub motion: usize,
    pub frame: f32,
    /// Stop on the last frame instead of looping.
    pub once: bool,
    /// One object turned further (object, rotation): a Marine's head.
    pub turn: Option<(usize, Quat)>,
}

impl Body {
    pub fn new(pos: Vec3, yaw: f32, model: usize, motions: Option<usize>) -> Self {
        Self { pos, yaw, model, motions, motion: 0, frame: 0.0, once: false, turn: None }
    }

    /// Switches to a looping motion, restarting it only when it changes.
    pub fn play(&mut self, motion: usize) {
        if self.motion != motion || self.once {
            self.motion = motion;
            self.frame = 0.0;
            self.once = false;
        }
    }

    /// Plays a motion once and holds its last frame.
    pub fn play_once(&mut self, motion: usize) {
        if self.motion != motion || !self.once {
            self.motion = motion;
            self.frame = 0.0;
            self.once = true;
        }
    }

    pub fn forward(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos())
    }
}

/// Turns `yaw` toward `target` by at most `max_step` radians.
pub fn turn_toward(yaw: f32, target: f32, max_step: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let mut diff = (target - yaw).rem_euclid(TAU);
    if diff > PI {
        diff -= TAU;
    }
    yaw + diff.clamp(-max_step, max_step)
}

/// A model carried in a character's hand (weapons), placed on that joint
/// every frame. Assumes the model is built in the hand's space.
#[derive(Component)]
pub struct Held {
    pub owner: Entity,
    /// Joint (object index) it is attached to.
    pub object: usize,
    /// Only shown while the owner is aiming (Snake's M9).
    pub when_aiming: bool,
    /// Weapon orientation in the hand's space (found by eye, not from the
    /// game: the models are built barrel along -y).
    pub grip: Quat,
    /// Offset (mm) in the joint's space (slung rifles; 0 in the hand).
    pub offset: Vec3,
    /// Shown only with this weapon in Snake's hand (0 M9, 1 USP).
    pub weapon: Option<u8>,
    /// Not shown (a soldier's radio shot to pieces).
    pub hidden: bool,
}

/// Grip for the AKS (barrel along -y in the model): points it forward (+z)
/// from the guard's hand.
pub fn rifle_grip() -> Quat {
    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
}

/// Grip for the M9: as modelled (it is built in the hand's space; the AKS's
/// turn above may mean the game holds the rifle from another joint).
pub fn pistol_grip() -> Quat {
    Quat::IDENTITY
}

/// Right hand joint of both Snake (`sna_def`) and the soldiers (`gbs_def`):
/// the end of the arm chain on the -x side (models face +z).
pub const RIGHT_HAND: usize = 6;

pub fn hold(
    mut commands: Commands,
    game: Res<Game>,
    inv: Res<crate::items::Inventory>,
    bodies: Query<(&Body, Option<&crate::player::Player>, Option<&Visibility>, Option<&EvmBody>), Without<Held>>,
    mut held: Query<(Entity, &Held, &mut Transform, &mut Visibility)>,
    children: Query<&Children>,
    parts: Query<&Mesh3d, With<Part>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut last: Local<std::collections::HashMap<Entity, Vec3>>,
) {
    let lt2 = game.data.lights.first().map(|(_, l)| l).filter(|_| std::env::var_os("MGS2_NO_LT2").is_none());
    for (e, h, mut t, mut vis) in &mut held {
        let Ok((b, player, owner_vis, evm)) = bodies.get(h.owner) else {
            // Its owner has gone (a trooper who left).
            commands.entity(e).try_despawn();
            continue;
        };
        let model = &game.data.models[b.model].1;
        let motion = b.motions.and_then(|m| game.data.motions[m].1.motions.get(b.motion));
        let joints = match evm.and_then(|x| game.data.evms.get(x.0)) {
            Some((_, em)) => evm_bones(em, b, &game),
            None => anim::pose(model, motion, b.frame, false),
        };
        let Some(hand) = joints.get(h.object).copied() else { continue };
        let owner = Transform::from_translation(b.pos * SCALE).with_rotation(Quat::from_rotation_y(b.yaw));
        let (_, rot, pos) = hand.to_scale_rotation_translation();
        *t = owner * Transform::from_translation((pos + rot * h.offset) * SCALE).with_rotation(rot * h.grip);
        let shown = !h.hidden && (player.is_some() || owner_vis != Some(&Visibility::Hidden)) && (!h.when_aiming || player.is_some_and(|p| p.aiming)) && h.weapon.is_none_or(|w| w == if inv.usp { inv.weapon } else { 0 });
        *vis = if shown { Visibility::Inherited } else { Visibility::Hidden };
        // Lit like its owner, again whenever it has moved 10 cm.
        if let Some(l) = lt2.filter(|_| shown)
            && last.get(&e).is_none_or(|p| p.distance(t.translation) > 0.1)
        {
            last.insert(e, t.translation);
            let at = t.translation / SCALE;
            let reach = Vec3::splat(1000.0);
            let lights = l.gather((at - reach).into(), (at + reach).into());
            for child in children.iter_descendants(e) {
                let Ok(m3) = parts.get(child) else { continue };
                let Some(mut mesh) = meshes.get_mut(&m3.0) else { continue };
                use bevy::mesh::VertexAttributeValues as V;
                let (Some(V::Float32x3(p)), Some(V::Float32x3(n))) = (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.attribute(Mesh::ATTRIBUTE_NORMAL)) else { continue };
                let col: Vec<[f32; 4]> = p
                    .iter()
                    .zip(n)
                    .map(|(p, n)| {
                        let c = l.shade((t.transform_point(Vec3::from(*p)) / SCALE).into(), (t.rotation * Vec3::from(*n)).into(), &lights);
                        [c[0] / 128.0, c[1] / 128.0, c[2] / 128.0, 1.0]
                    })
                    .collect();
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
            }
        }
    }
}

/// One mesh of a body: every kms mesh with the same texture, merged so a
/// body updates a few mesh assets a frame instead of dozens.
#[derive(Component)]
pub struct BodyMesh {
    /// (object, mesh) of each merged kms mesh, in vertex order.
    segs: Vec<(usize, usize)>,
}

/// Spawns a character model as one merged mesh per texture.
#[allow(clippy::too_many_arguments)]
pub fn spawn_body(
    commands: &mut Commands,
    model: &mgs2_formats::kms::Model,
    data: &mgs2_bevy::stage::StageData,
    cache: &mut mgs2_bevy::scene::MaterialCache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    unlit: bool,
    root: impl Bundle,
) -> Entity {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues as V};
    let parent = commands.spawn((root, Transform::default(), Visibility::default())).id();
    let mut groups: Vec<(u32, Vec<(Mesh, Part)>)> = Vec::new();
    for (tex, mesh, part) in mgs2_bevy::scene::model_meshes(model) {
        match groups.iter_mut().find(|(t, _)| *t == tex) {
            Some((_, g)) => g.push((mesh, part)),
            None => groups.push((tex, vec![(mesh, part)])),
        }
    }
    for (tex, group) in groups {
        let (mut pos, mut nrm, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut segs = Vec::new();
        for (m, part) in &group {
            let base = pos.len() as u32;
            if let Some(V::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) {
                pos.extend_from_slice(p);
            }
            if let Some(V::Float32x3(n)) = m.attribute(Mesh::ATTRIBUTE_NORMAL) {
                nrm.extend_from_slice(n);
            }
            if let Some(V::Float32x2(u)) = m.attribute(Mesh::ATTRIBUTE_UV_0) {
                uv.extend_from_slice(u);
            }
            if let Some(Indices::U32(i)) = m.indices() {
                idx.extend(i.iter().map(|i| base + i));
            }
            segs.push((part.object, part.mesh));
        }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
        mesh.insert_indices(Indices::U32(idx));
        let mat = cache.get(tex, data, images, materials, unlit);
        let child = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat), BodyMesh { segs })).id();
        commands.entity(parent).add_child(child);
    }
    parent
}

/// Places every body and CPU-skins its meshes.
pub fn skin(
    time: Res<Time>,
    game: Res<Game>,
    mut bodies: Query<(Entity, &mut Body, &mut Transform)>,
    children: Query<&Children>,
    parts: Query<(&BodyMesh, &Mesh3d, &ViewVisibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    cams: Query<&GlobalTransform, With<crate::player::FollowCam>>,
    mut last: Local<std::collections::HashMap<Entity, SkinKey>>,
) {
    let dt = time.delta_secs().min(0.05);
    last.retain(|e, _| bodies.contains(*e));
    let eye = cams.single().map_or(Vec3::ZERO, |c| c.translation() / SCALE);
    // Lit by the stage's lt2 like the map (assumed: the same per-vertex
    // lights at the body's current place).
    let lt2 = game.data.lights.first().map(|(_, l)| l).filter(|_| std::env::var_os("MGS2_NO_LT2").is_none());
    // Bodies to skin this frame, with their parts.
    let mut jobs: Vec<(&Body, f32, Quat, Vec<(&BodyMesh, AssetId<Mesh>)>)> = Vec::new();
    let mut to_skin: Vec<Entity> = Vec::new();
    for (e, mut b, mut t) in &mut bodies {
        t.translation = b.pos * SCALE;
        t.rotation = Quat::from_rotation_y(b.yaw);
        let motion = b.motions.and_then(|m| game.data.motions[m].1.motions.get(b.motion));
        b.frame += dt * 60.0;
        if let (true, Some(m)) = (b.once, motion) {
            b.frame = b.frame.min(m.length.saturating_sub(1) as f32);
        }
        // Off screen last frame: only the clock runs.
        if !children.iter_descendants(e).any(|c| parts.get(c).is_ok_and(|p| p.2.get())) {
            continue;
        }
        // Motions are keyed at 60 frames a second, as the game plays them:
        // skin again only when the whole frame, the place or the facing
        // changes (most frames at higher refresh rates; still crowds).
        let key = SkinKey::of(&b, step(b.pos.distance(eye)));
        if last.get(&e) == Some(&key) {
            continue;
        }
        last.insert(e, key);
        to_skin.push(e);
    }
    for (e, b, t) in bodies.iter_many(&to_skin) {
        let list = children.iter_descendants(e).filter_map(|c| parts.get(c).ok().map(|(p, m, _)| (p, m.0.id()))).collect();
        jobs.push((b, last[&e].frame as f32, t.rotation, list));
    }
    // Skinning and lighting run in parallel; the meshes are written after.
    let pool = bevy::tasks::ComputeTaskPool::get();
    let game = &*game;
    let results: Vec<Vec<(AssetId<Mesh>, Vec<[f32; 3]>, Vec<[f32; 3]>, Option<Vec<[f32; 4]>>)>> = pool.scope(|scope| {
        for (b, frame, rot, list) in &jobs {
            scope.spawn(async move {
                let model = &game.data.models[b.model].1;
                let motion = b.motions.and_then(|m| game.data.motions[m].1.motions.get(b.motion));
                let mats = anim::pose_turned(model, motion, *frame, false, b.turn);
                let reach = Vec3::splat(2000.0);
                let lights = lt2.map(|l| l.gather((b.pos - reach).into(), (b.pos + reach).into())).unwrap_or_default();
                list.iter()
                    .map(|(part, id)| {
                        let (mut pos, mut nrm) = (Vec::new(), Vec::new());
                        for &(o, m) in &part.segs {
                            let (p, n) = anim::skin(model, o, m, &mats);
                            pos.extend(p);
                            nrm.extend(n);
                        }
                        let col = lt2.map(|l| {
                            pos.iter()
                                .zip(&nrm)
                                .map(|(p, n)| {
                                    let c = l.shade((*rot * Vec3::from(*p) + b.pos).into(), (*rot * Vec3::from(*n)).into(), &lights);
                                    [c[0] / 128.0, c[1] / 128.0, c[2] / 128.0, 1.0]
                                })
                                .collect()
                        });
                        let pos = pos.into_iter().map(|v| (Vec3::from(v) * SCALE).into()).collect();
                        (*id, pos, nrm, col)
                    })
                    .collect()
            });
        }
    });
    BODIES.fetch_add(results.len(), std::sync::atomic::Ordering::Relaxed);
    for (id, pos, nrm, col) in results.into_iter().flatten() {
        let Some(mut mesh) = meshes.get_mut(id) else { continue };
        STATS.fetch_add(pos.len(), std::sync::atomic::Ordering::Relaxed);
        if let Some(col) = col {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
        }
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    }
}

pub static STATS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub static BODIES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// What a body was last skinned with.
#[derive(Clone, Copy, PartialEq)]
pub struct SkinKey {
    model: usize,
    motions: Option<usize>,
    motion: usize,
    frame: i32,
    pos: [i32; 3],
    yaw: u32,
    turn: Option<(usize, [u32; 4])>,
}

/// Animation frames per skinning update by distance from the camera (mm):
/// every frame close up, every second or fourth further away (re-uploading
/// skinned meshes is what costs, with crowds of Marines in the holds).
pub fn step(distance: f32) -> i32 {
    if distance < 6000.0 {
        1
    } else if distance < 15000.0 {
        2
    } else {
        4
    }
}

impl SkinKey {
    fn of(b: &Body, step: i32) -> Self {
        Self {
            model: b.model,
            motions: b.motions,
            motion: b.motion,
            frame: (b.frame.floor() as i32).div_euclid(step) * step,
            pos: b.pos.round().as_ivec3().to_array(),
            yaw: b.yaw.to_bits(),
            turn: b.turn.map(|(j, q)| (j, q.to_array().map(f32::to_bits))),
        }
    }
}

/// A character's shadow on the floor under it. The game links a low-poly
/// shadow model to the body (`sna_shadow` for Snake, joined when the object
/// counts match, `FUN_00129448`) and draws it as a shadow volume; here it
/// is posed like the body and flattened onto the floor at its feet, drawn
/// translucent black (an approximation: straight down, flat floors).
#[derive(Component)]
pub struct DropShadow {
    model: usize,
}

/// Shadow models by body model name.
const SHADOW_MODELS: &[(&str, &str)] = &[("sna_def", "sna_shadow")];

/// Gives every new body its shadow.
pub fn add_shadows(
    mut commands: Commands,
    game: Res<Game>,
    added: Query<(Entity, &Body), Added<Body>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut material: Local<Option<Handle<StandardMaterial>>>,
) {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    use mgs2_formats::str_code;
    if std::env::var_os("MGS2_NO_SHADOW").is_some() {
        return;
    }
    for (e, b) in &added {
        let data = &game.data;
        let body_hash = data.models[b.model].0;
        let objects = data.models[b.model].1.objects.len();
        let model = SHADOW_MODELS
            .iter()
            .filter(|(body, _)| str_code(body.as_bytes()) == body_hash)
            .find_map(|(_, s)| data.models.iter().position(|(h, m)| *h == str_code(s.as_bytes()) && m.objects.len() == objects))
            .unwrap_or(b.model);
        let mat = material
            .get_or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: Color::srgba(0.0, 0.0, 0.0, 0.45),
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                })
            })
            .clone();
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; 3]);
        mesh.insert_indices(Indices::U32(vec![0, 1, 2]));
        let child = commands
            .spawn((DropShadow { model }, Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat), Transform::default(), Visibility::default()))
            .id();
        commands.entity(e).add_child(child);
    }
}

/// Poses each shadow like its body and flattens it onto the floor.
pub fn update_shadows(
    game: Res<Game>,
    bodies: Query<&Body>,
    shadows: Query<(Entity, &DropShadow, &ChildOf, &Mesh3d, &ViewVisibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    cams: Query<&GlobalTransform, With<crate::player::FollowCam>>,
    mut last: Local<std::collections::HashMap<Entity, (usize, Option<usize>, usize, i32)>>,
) {
    let eye = cams.single().map_or(Vec3::ZERO, |c| c.translation() / SCALE);
    last.retain(|e, _| shadows.contains(*e));
    use bevy::mesh::Indices;
    /// Above the floor, against z-fighting (mm).
    const LIFT: f32 = 15.0;
    for (e, s, parent, m3, seen) in &shadows {
        let Ok(b) = bodies.get(parent.parent()) else { continue };
        if !seen.get() {
            continue;
        }
        // The flattened pose only changes with the motion frame.
        let step = step(b.pos.distance(eye));
        let frame = (b.frame.floor() as i32).div_euclid(step) * step;
        let key = (s.model, b.motions, b.motion, frame);
        if last.get(&e) == Some(&key) {
            continue;
        }
        last.insert(e, key);
        let model = &game.data.models[s.model].1;
        let motion = b.motions.and_then(|m| game.data.motions[m].1.motions.get(b.motion));
        let mats = anim::pose(model, motion, frame as f32, false);
        let mut pos: Vec<[f32; 3]> = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        for (o, obj) in model.objects.iter().enumerate() {
            for (mi, mesh) in obj.meshes.iter().enumerate() {
                if mesh.indices.is_empty() {
                    continue;
                }
                let base = pos.len() as u32;
                let (p, _) = anim::skin(model, o, mi, &mats);
                pos.extend(p.into_iter().map(|v| [v[0] * SCALE, LIFT * SCALE, v[2] * SCALE]));
                idx.extend(mesh.indices.iter().map(|i| base + i));
            }
        }
        let Some(mut mesh) = meshes.get_mut(&m3.0) else { continue };
        let n = pos.len();
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n]);
        mesh.insert_indices(Indices::U32(idx));
    }
}

/// A body drawn with an `.evm` skinned mesh (index into
/// `StageData::evms`) in place of its kms, which only gives the skeleton
/// for the rest of the game (the commandant's `sco_def` has no kms). Its
/// first 21 bones are the standard body, turned by the motion's joints.
#[derive(Component)]
pub struct EvmBody(pub usize);

/// One mesh of an [`EvmBody`].
#[derive(Component)]
pub struct EvmBodyPart(usize);

/// Spawns an evm character, one mesh per evm mesh.
#[allow(clippy::too_many_arguments)]
pub fn spawn_evm_body(
    commands: &mut Commands,
    em: &mgs2_formats::evm::Model,
    data: &mgs2_bevy::stage::StageData,
    cache: &mut mgs2_bevy::scene::MaterialCache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    unlit: bool,
    root: impl Bundle,
) -> Entity {
    let parent = commands.spawn((root, Transform::default(), Visibility::default())).id();
    for (tex, mesh, i) in crate::demo::evm_meshes(em) {
        let mat = cache.get(tex, data, images, materials, unlit);
        let child = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat), EvmBodyPart(i))).id();
        commands.entity(parent).add_child(child);
    }
    parent
}

/// Bone matrices (mm, body space) of an evm body at its motion's frame:
/// the hips stand as high above the feet as the bind pose's lowest bones
/// lie below them, raised by the motion's root track as a kms body's are.
pub fn evm_bones(em: &mgs2_formats::evm::Model, b: &Body, game: &Game) -> Vec<bevy::math::Affine3A> {
    let motion = b.motions.and_then(|m| game.data.motions[m].1.motions.get(b.motion));
    let root_y = motion.and_then(|m| m.root_position(b.frame)).map_or(0.0, |p| p[1]);
    let hips = -em.bones.iter().map(|x| x.position[1]).fold(0.0, f32::min);
    let rots: Vec<Quat> = (0..em.animated.min(em.bones.len()))
        .map(|i| {
            let r = motion.and_then(|m| m.rotation(i, b.frame)).map_or(Quat::IDENTITY, |q| Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize());
            match b.turn {
                Some((j, t)) if j == i => r * t,
                _ => r,
            }
        })
        .collect();
    crate::demo::evm_pose(em, &rots, Some(Vec3::new(0.0, hips + root_y, 0.0)))
}

/// Skins the evm bodies on screen (after [`skin`], which places them and
/// runs their clocks), lit by the stage's lt2 as the kms bodies are.
pub fn skin_evm(
    game: Res<Game>,
    bodies: Query<(Entity, &Body, &EvmBody, &Transform)>,
    children: Query<&Children>,
    parts: Query<(&EvmBodyPart, &Mesh3d, &ViewVisibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut last: Local<std::collections::HashMap<Entity, SkinKey>>,
) {
    let lt2 = game.data.lights.first().map(|(_, l)| l).filter(|_| std::env::var_os("MGS2_NO_LT2").is_none());
    for (e, b, evm, t) in &bodies {
        let Some((_, em)) = game.data.evms.get(evm.0) else { continue };
        let list: Vec<_> = children.iter_descendants(e).filter_map(|c| parts.get(c).ok()).collect();
        // Off screen: skinned again once seen (the first frame always).
        let key = SkinKey::of(b, 1);
        if last.get(&e) == Some(&key) || (last.contains_key(&e) && !list.iter().any(|p| p.2.get())) {
            continue;
        }
        last.insert(e, key);
        let mats = evm_bones(em, b, &game);
        let reach = Vec3::splat(2000.0);
        let lights = lt2.map(|l| l.gather((b.pos - reach).into(), (b.pos + reach).into())).unwrap_or_default();
        for (part, m3, _) in list {
            let Some(src) = em.meshes.get(part.0) else { continue };
            let Some(mut mesh) = meshes.get_mut(&m3.0) else { continue };
            let (pos, nrm) = crate::demo::evm_skin(em, src, &mats);
            if let Some(l) = lt2 {
                let col: Vec<[f32; 4]> = pos
                    .iter()
                    .zip(&nrm)
                    .map(|(p, n)| {
                        let c = l.shade((t.rotation * Vec3::from(*p) + b.pos).into(), (t.rotation * Vec3::from(*n)).into(), &lights);
                        [c[0] / 128.0, c[1] / 128.0, c[2] / 128.0, 1.0]
                    })
                    .collect();
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
            }
            let pos: Vec<[f32; 3]> = pos.into_iter().map(|v| (Vec3::from(v) * SCALE).into()).collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
        }
    }
}
