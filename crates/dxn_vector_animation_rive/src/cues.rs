//! Rive events as the scene's cues: an event's name, an open-URL event's
//! link and target, and its custom properties, as Rive's web runtime
//! reports them (`createRiveEventObject`).

use dxn_vector_animation::{Cue, CueValue};

use crate::{
    defs::{props as p, types as t},
    file::Object,
};

/// The key of an event's `trigger`, which animations key to fire it (a
/// callback, which the generated tables leave out).
pub(crate) const EVENT_TRIGGER: u16 = 395;

/// Event `event` of the artboard whose objects are `objects`, as a cue at
/// `time`; `None` when it is not an event.
pub(crate) fn cue(objects: &[Option<Object>], event: usize, time: f64) -> Option<Cue> {
    let o = objects.get(event)?.as_ref()?;
    if !o.is(t::EVENT) {
        return None;
    }
    let url = (o.ty == t::OPEN_URL_EVENT).then(|| {
        let target = match o.uint(p::OPEN_URL_EVENT_TARGET_VALUE) {
            1 => "_parent",
            2 => "_self",
            3 => "_top",
            _ => "_blank",
        };
        (o.string(p::OPEN_URL_EVENT_URL), target.to_owned())
    });
    let properties = objects
        .iter()
        .flatten()
        .filter(|c| c.is(t::CUSTOM_PROPERTY) && c.id(p::COMPONENT_PARENT_ID) == Some(event))
        .filter_map(|c| {
            let name = c.string(p::COMPONENT_NAME);
            let value = match c.ty {
                t::CUSTOM_PROPERTY_NUMBER => {
                    CueValue::Number(c.num(p::CUSTOM_PROPERTY_NUMBER_PROPERTY_VALUE))
                }
                t::CUSTOM_PROPERTY_STRING => {
                    CueValue::Text(c.string(p::CUSTOM_PROPERTY_STRING_PROPERTY_VALUE))
                }
                t::CUSTOM_PROPERTY_BOOLEAN => {
                    CueValue::Bool(c.flag(p::CUSTOM_PROPERTY_BOOLEAN_PROPERTY_VALUE))
                }
                _ => return None,
            };
            (!name.is_empty()).then_some((name, value))
        })
        .collect();
    Some(Cue {
        time,
        name: o.string(p::COMPONENT_NAME),
        url,
        properties,
    })
}
