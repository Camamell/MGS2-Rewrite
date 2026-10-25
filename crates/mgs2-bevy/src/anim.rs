//! CPU skinning of kms models posed by mar motions.

use bevy::math::{Affine3A, Quat, Vec3};
use mgs2_formats::kms;
use mgs2_formats::mar::Motion;

/// Object flag: drawn on its parent's matrix (its offset repeats the
/// parent's).
pub const OVERLAY: u32 = 0x4000;

/// Model-space matrix (millimetres) of every object. Joints without a track
/// keep their bind orientation. The motion's root track supplies the hips
/// height (horizontal root motion is left to the caller).
pub fn pose(model: &kms::Model, motion: Option<&Motion>, frame: f32, conjugate: bool) -> Vec<Affine3A> {
    pose_turned(model, motion, frame, conjugate, None)
}

/// [`pose`] with one object turned further by `turn` (in its own space,
/// after the motion's rotation): a head looking about.
pub fn pose_turned(model: &kms::Model, motion: Option<&Motion>, frame: f32, conjugate: bool, turn: Option<(usize, Quat)>) -> Vec<Affine3A> {
    pose_mirrored(model, motion, frame, conjugate, turn, false)
}

/// Left and right joints the game swaps to mirror a pose (`FUN_003b2f58`,
/// tables at 0x1fc070..0x1fc0ac of the stage image): 16-joint skeletons
/// (model flags 0x10000) and 21-joint ones (0x20000, soldiers, Olga).
const MIRROR_16: [(usize, usize); 6] = [(7, 2), (8, 3), (9, 4), (13, 10), (14, 11), (15, 12)];
const MIRROR_21: [(usize, usize); 8] = [(7, 3), (8, 4), (9, 5), (10, 6), (17, 13), (18, 14), (19, 15), (20, 16)];

/// [`pose_turned`], optionally mirrored left for right as the game plays a
/// motion under model flags 0x300 (`FUN_003b2f58`): every joint's
/// rotation has x and w negated (`vsub.xw` from zero: the reflection in
/// the model's x = 0 plane) and the paired arm and leg joints trade
/// rotations. Skeletons of other sizes are left as they are.
pub fn pose_mirrored(model: &kms::Model, motion: Option<&Motion>, frame: f32, conjugate: bool, turn: Option<(usize, Quat)>, mirror: bool) -> Vec<Affine3A> {
    let root_y = motion.and_then(|m| m.root_position(frame)).map_or(0.0, |p| p[1]);
    let mut rots: Vec<Option<[f32; 4]>> = (0..model.objects.len()).map(|i| motion.and_then(|m| m.rotation(i, frame))).collect();
    if mirror {
        let (joints, pairs): (usize, &[(usize, usize)]) = match rots.len() {
            21.. => (21, &MIRROR_21),
            16..=20 => (16, &MIRROR_16),
            _ => (0, &[]),
        };
        for q in rots.iter_mut().take(joints).flatten() {
            q[0] = -q[0];
            q[3] = -q[3];
        }
        for &(a, b) in pairs {
            rots.swap(a, b);
        }
    }
    let mut out: Vec<Affine3A> = Vec::with_capacity(model.objects.len());
    for (i, o) in model.objects.iter().enumerate() {
        let rot = rots[i]
            .map(|q| {
                let q = Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize();
                if conjugate { q.conjugate() } else { q }
            })
            .unwrap_or(Quat::IDENTITY);
        let rot = match turn {
            Some((j, t)) if j == i => rot * t,
            _ => rot,
        };
        let mut offset = Vec3::from(o.offset);
        if o.parent < 0 {
            offset += Vec3::from(model.origin);
            offset.y += root_y;
        }
        let parent = usize::try_from(o.parent).ok().and_then(|p| out.get(p).copied());
        // Overlay objects (flag 0x4000: Snake's hair layer, the Marines'
        // sleeves...) repeat their parent's offset and sit exactly on it.
        if o.flags & OVERLAY != 0
            && let Some(p) = parent
        {
            out.push(p);
            continue;
        }
        let local = Affine3A::from_rotation_translation(rot, offset);
        out.push(parent.unwrap_or(Affine3A::IDENTITY) * local);
    }
    out
}

/// Model-space matrices from explicit joint rotations (a cutscene's actor
/// stream): joint i turns object i; `root`, if given, replaces the root
/// object's offset (the hips' position).
pub fn pose_joints(model: &kms::Model, rotations: &[Quat], root: Option<Vec3>) -> Vec<Affine3A> {
    let mut out: Vec<Affine3A> = Vec::with_capacity(model.objects.len());
    for (i, o) in model.objects.iter().enumerate() {
        let rot = rotations.get(i).copied().unwrap_or(Quat::IDENTITY);
        let mut offset = Vec3::from(o.offset);
        if o.parent < 0 {
            offset = root.unwrap_or(offset + Vec3::from(model.origin));
        }
        let parent = usize::try_from(o.parent).ok().and_then(|p| out.get(p).copied());
        // Overlay objects (flag 0x4000: Snake's hair layer, the Marines'
        // sleeves...) repeat their parent's offset and sit exactly on it.
        if o.flags & OVERLAY != 0
            && let Some(p) = parent
        {
            out.push(p);
            continue;
        }
        let local = Affine3A::from_rotation_translation(rot, offset);
        out.push(parent.unwrap_or(Affine3A::IDENTITY) * local);
    }
    out
}

/// Skinned positions (millimetres) and normals of one mesh:
/// `w * M_self * v + (1 - w) * M_parent * (v + offset_self)`.
pub fn skin(model: &kms::Model, obj: usize, mesh: usize, mats: &[Affine3A]) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let o = &model.objects[obj];
    let m = &o.meshes[mesh];
    let me = mats[obj];
    let parent = usize::try_from(o.parent).ok().and_then(|p| mats.get(p).copied());
    let off = Vec3::from(o.offset);
    let mut pos = Vec::with_capacity(m.positions.len());
    let mut nrm = Vec::with_capacity(m.positions.len());
    for (i, p) in m.positions.iter().enumerate() {
        let v = Vec3::from(*p);
        let w = (f32::from(m.position_w[i]) / 4096.0).clamp(0.0, 1.0);
        let a = me.transform_point3(v);
        let (world, rot) = match parent {
            Some(pm) if w < 1.0 => {
                let b = pm.transform_point3(v + off);
                (a * w + b * (1.0 - w), if w >= 0.5 { me } else { pm })
            }
            _ => (a, me),
        };
        pos.push(world.into());
        let n = m.normals.get(i).map_or(Vec3::Y, |n| Vec3::from(*n));
        nrm.push(rot.transform_vector3(n).normalize_or(Vec3::Y).into());
    }
    (pos, nrm)
}
