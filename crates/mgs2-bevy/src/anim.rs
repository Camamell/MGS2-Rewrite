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
    let root_y = motion.and_then(|m| m.root_position(frame)).map_or(0.0, |p| p[1]);
    let mut out: Vec<Affine3A> = Vec::with_capacity(model.objects.len());
    for (i, o) in model.objects.iter().enumerate() {
        let rot = motion
            .and_then(|m| m.rotation(i, frame))
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
