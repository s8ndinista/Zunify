use ferroui_base::{ObjectType, Ref};
use ferroui_view::El;

/// Build a static view for Zunify's existing control lifecycle. Use `with` for
/// callbacks that belong to the control, and let the builder release its
/// construction state once the tree is populated.
pub(crate) fn mount<T: ObjectType>(element: El<T>) -> Ref<T> {
    let (control, owner) = element.build().mount_root();
    owner.dispose();
    control
        .downcast::<T>()
        .ok()
        .expect("builder returned its control type")
}
