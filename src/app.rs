use crate::effects;

use ferroui_base::{
    BoxedValue, CornerRadius, FerroObjectImpl, Ref, RelativePoint, RelativeUnit, Thickness,
    WeakRef,
    data::{BindingMode, ReflectionBinding, TemplateBinding},
    ferro_class, ferro_impl_classes,
    input::{InputElement, gesture_recognizers::ScrollGestureRecognizer},
    instantiate,
    layout::{HorizontalAlignment as H, Orientation, VerticalAlignment as V},
    media::{
        Color, FontManagerOptions, FontWeight, Geometry, GradientStop, IBrush, LinearGradientBrush,
        Stretch, TextTrimming, TextWrapping, effects::ImmutableDropShadowEffect, imaging::Bitmap,
        immutable::ImmutableSolidColorBrush,
    },
    styling::ThemeVariant,
};
#[cfg(target_os = "emscripten")]
use ferroui_browser::{BrowserAccessibilityMode, BrowserAppBuilder, BrowserPlatformOptions};
use ferroui_controls::{
    AppBuilder, Application, ApplicationImpl, ApplicationImplExt, Border, Button,
    ColumnDefinitions, ContentControl, Control, Grid, Image, ItemsSource, NewApplication,
    RepeatButton, RowDefinitions, ScrollViewer, Slider, StackPanel, TextBlock, TextBox, ToolTip,
    automation::AutomationProperties,
    media::{
        ApplicationTheme, RevealBackgroundBrush, RevealBorderBrush, RevealBrush, RevealBrushState,
    },
    presenters::{ContentPresenter, ScrollContentPresenter},
    primitives::{RangeBase, ScrollBar, ScrollBarVisibility, TemplatedControl, Thumb, Track},
    shapes::Path,
    templates::{FuncControlTemplate, FuncDataTemplate, FuncTemplateNameScopeExtensions},
};
use ferroui_controls_repeater::{ItemsRepeater, StackLayout};
#[cfg(not(target_os = "emscripten"))]
use ferroui_desktop::AppBuilderDesktopExtensions as _;
use ferroui_fonts_inter::AppBuilderExtension;
use ferroui_themes_winui::WinUITheme;
use serde::Deserialize;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    io::Cursor,
    rc::Rc,
};
#[cfg(target_os = "emscripten")]
use wasm_bindgen::prelude::*;

const APP_NAME: &str = "Zunify";
const BG: &str = "#130E15";
const PANEL: &str = "#1E1720";
const EDGE: &str = "#382C37";
const TEXT: &str = "#F8F4F7";
const MUTED: &str = "#BBAFBA";
const DIM: &str = "#A196A0";
const ACCENT: &str = "#FF3595";

#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Item {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) album: String,
    pub(crate) art: String,
    pub(crate) duration: f64,
    pub(crate) kind: String,
    pub(crate) liked: bool,
}
#[cfg(not(target_os = "emscripten"))]
impl Item {
    pub(crate) fn record_in(&self, recent: &mut Vec<Self>) {
        if self.id.is_empty() || recent.first().is_some_and(|item| item.id == self.id) {
            return;
        }
        recent.retain(|item| item.id != self.id);
        recent.insert(0, self.clone());
        recent.truncate(6);
    }
}
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Model {
    pub(crate) page: String,
    pub(crate) title: String,
    pub(crate) caption: String,
    pub(crate) detail: String,
    pub(crate) query: String,
    pub(crate) status: String,
    pub(crate) connected: bool,
    pub(crate) items: Vec<Item>,
    pub(crate) collections: Vec<Item>,
    pub(crate) queue: Vec<Item>,
    pub(crate) pins: Vec<Item>,
    pub(crate) recent: Vec<Item>,
    pub(crate) current: Item,
    pub(crate) playing: bool,
    pub(crate) shuffle: bool,
    pub(crate) repeat: bool,
    pub(crate) volume: f64,
    #[cfg(not(target_os = "emscripten"))]
    pub(crate) connecting: bool,
    #[cfg(not(target_os = "emscripten"))]
    pub(crate) client_id: String,
}
#[derive(Default)]
struct Ui {
    model: Model,
    width: f64,
    reduced: bool,
    shell: Option<Ref<Border>>,
    #[cfg(target_os = "emscripten")]
    reveal: Option<WeakRef<Border>>,
    body: Option<Ref<Border>>,
    main: Option<Ref<ScrollViewer>>,
    collection: Option<Rc<CollectionView>>,
    nav: Vec<(&'static str, Ref<TextBlock>)>,
    footer: Option<Ref<Border>>,
    chrome: Option<Ref<Border>>,
    transport: Option<Ref<Border>>,
    fire: Option<Rc<effects::TransportFire>>,
    account_label: Option<Ref<TextBlock>>,
    artist_filter: Option<String>,
    now_stage: Option<WeakRef<Grid>>,
    search: Option<Ref<TextBox>>,
    seek: Option<Ref<Slider>>,
    elapsed: Option<Ref<TextBlock>>,
    art: HashMap<String, Rc<Bitmap>>,
    waiting: HashMap<String, Vec<WeakRef<Image>>>,
    art_order: std::collections::VecDeque<String>,
}
thread_local! {
    static UI: RefCell<Ui> = RefCell::default();
    static LIGHT_PAGE: Cell<bool> = const { Cell::new(false) };
    static TICKING: Cell<bool> = const { Cell::new(false) };
}

#[cfg(target_os = "emscripten")]
#[wasm_bindgen(
    inline_js = "export function send(action, data) { globalThis.spotifyFerroAction?.(action, data); }"
)]
extern "C" {
    fn send(action: &str, data: &str);
}

#[cfg(not(target_os = "emscripten"))]
fn send(action: &str, data: &str) {
    use ferroui_base::threading::{Dispatcher, DispatcherPriority};
    let (action, data) = (action.to_owned(), data.to_owned());
    Dispatcher::ui_thread().post_local(
        move || crate::desktop::dispatch(&action, &data),
        DispatcherPriority::NORMAL,
    );
}

#[repr(C)]
pub struct App {
    base: Application,
}
ferro_class!(App: Application);
ferro_impl_classes!(App: FerroObjectImpl);
impl NewApplication for App {
    fn new_application() -> Ref<Self> {
        instantiate(Self {
            base: Application::construct(),
        })
    }
}
impl ApplicationImpl for App {
    fn initialize(this: &Self) {
        Self::parent_initialize(this);
        this.set_name(Some(APP_NAME.into()));
        this.set_requested_theme_variant(Some(ThemeVariant::dark()));
        this.styles().add(WinUITheme::new().as_style());
        for (key, ink) in [
            ("SliderTrackValueFill", ACCENT),
            ("SliderTrackValueFillPointerOver", "#FF86BA"),
            ("SliderTrackValueFillPressed", "#DA256D"),
            ("SliderThumbBackground", ACCENT),
            ("SliderThumbBackgroundPointerOver", "#FF86BA"),
            ("SliderThumbBackgroundPressed", "#DA256D"),
        ] {
            this.resources().set(key, Some(Rc::new(brush(ink))));
        }
    }
    fn on_framework_initialization_completed(this: &Self) {
        let lifetime = this.application_lifetime();
        #[cfg(target_os = "emscripten")]
        if let Some(view) = lifetime
            .as_ref()
            .and_then(|l| l.as_single_view_application_lifetime())
        {
            view.set_main_view(Some(create_shell().upcast()));
        }
        #[cfg(not(target_os = "emscripten"))]
        if let Some(desktop) = lifetime
            .as_ref()
            .and_then(|l| l.as_classic_desktop_style_application_lifetime())
        {
            use ferroui_base::threading::{Dispatcher, DispatcherPriority};
            use ferroui_controls::Window;
            let shell = create_shell();
            shell.size_changed(|_, event| {
                let width = event.new_size().width;
                if event.width_changed() && width > 0.0 {
                    Dispatcher::ui_thread()
                        .post_local(move || resize(width), DispatcherPriority::NORMAL);
                }
            });
            let window = Window::new();
            window.set_title(Some(APP_NAME.into()));
            window.set_width(1350.0);
            window.set_height(900.0);
            window.set_min_width(380.0);
            window.set_min_height(450.0);
            install_desktop_title_bar(&window, shell);
            crate::desktop::window_ready(&window);
            desktop.set_main_window(Some(window));
        }
        Self::parent_on_framework_initialization_completed(this);
    }
}

#[cfg(not(target_os = "emscripten"))]
fn install_desktop_title_bar(window: &Ref<ferroui_controls::Window>, shell: Ref<Border>) {
    use ferroui_controls::{
        TitleBar, Window, WindowState,
        chrome::{TitleBarDecorations, WindowDrawnDecorations},
    };

    const TITLE_BAR_HEIGHT: f64 = 32.0; // WinUI's compact title bar height in DIPs.

    let title_bar = TitleBar::new();
    title_bar.set_title(APP_NAME);
    title_bar.set_background(Some(brush("#241A25")));
    title_bar.resources().set(
        "TitleBarTitleMargin",
        Some(Rc::new(Thickness::new(10.0, 0.0, 8.0, 0.0))),
    );

    let root = grid("", "Auto,*");
    root.set_clip_to_bounds(true);
    put(&root, 0, 0, title_bar.clone().upcast());
    put(&root, 0, 1, shell.upcast());
    if !UI.with(|ui| ui.borrow().reduced) {
        add_reveal_light(&root, 2);
    }

    window.set_extend_client_area_title_bar_height_hint(TITLE_BAR_HEIGHT);
    window.set_extend_client_area_to_decorations_hint(true);
    window.set_content(Some(Control::boxed(root.clone())));

    let decorations = WindowDrawnDecorations::get_title_bar_decorations(window);
    let (window_ref, root_ref, bar_ref) = (window.downgrade(), root.downgrade(), title_bar.downgrade());
    let update_chrome = move || {
        let (Some(window), Some(root), Some(bar)) =
            (window_ref.upgrade(), root_ref.upgrade(), bar_ref.upgrade())
        else {
            return;
        };
        let show_title = window.is_extended_into_window_decorations()
            && window.window_state() != WindowState::FullScreen;
        let margin = window.window_decoration_margin();
        bar.set_is_visible(show_title);
        if show_title {
            bar.set_min_height(margin.top);
            root.set_margin(Thickness::new(margin.left, 0.0, margin.right, margin.bottom));
            WindowDrawnDecorations::set_title_bar_decorations(
                &window,
                decorations & !TitleBarDecorations::TITLE,
            );
        } else {
            root.set_margin(margin);
            WindowDrawnDecorations::set_title_bar_decorations(&window, decorations);
        }
    };
    update_chrome();
    window.add_property_changed_handler(Rc::new(move |change| {
        let property = change.property();
        if property == Window::is_extended_into_window_decorations_property().as_property()
            || property == Window::window_decoration_margin_property().as_property()
            || property == Window::window_state_property().as_property()
        {
            update_chrome();
        }
    }));
}

fn create_shell() -> Ref<Border> {
    let host = surface(BG, 0.0, 0.0);
    UI.with(|ui| ui.borrow_mut().shell = Some(host.clone()));
    build();
    host
}

fn color(s: &str) -> Color {
    let ink = if LIGHT_PAGE.get() {
        match s {
            TEXT => "#211C23",
            MUTED | DIM => "#635A63",
            PANEL => "#F2EEF1",
            EDGE => "#DED7DE",
            BG => "#FCFAFC",
            _ => s,
        }
    } else {
        s
    };
    Color::parse(ink).unwrap()
}
fn brush(s: &str) -> Rc<dyn IBrush> {
    Rc::new(ImmutableSolidColorBrush::new(color(s)))
}
fn label(s: &str, size: f64, ink: &str) -> Ref<TextBlock> {
    let t = TextBlock::new();
    t.set_text(Some(s));
    t.set_font_size(size);
    t.set_foreground(Some(brush(ink)));
    t.set_text_trimming(<dyn TextTrimming>::character_ellipsis());
    t
}
fn metro_title(text: &str, size: f64) -> Ref<TextBlock> {
    let title = label(text, size, TEXT);
    title.set_font_weight(FontWeight::Light);
    title.set_letter_spacing(-0.9);
    title
}

fn strong(s: &str, size: f64, ink: &str) -> Ref<TextBlock> {
    let t = label(s, size, ink);
    t.set_font_weight(FontWeight::SemiBold);
    t
}
fn kicker(s: &str, ink: &str) -> Ref<TextBlock> {
    let t = strong(s, 10.0, ink);
    t.set_letter_spacing(1.65);
    t
}
fn surface(bg: &str, pad: f64, radius: f64) -> Ref<Border> {
    let b = Border::new();
    b.set_background(Some(brush(bg)));
    b.set_corner_radius(CornerRadius::uniform(radius));
    b.set_padding(Thickness::uniform(pad));
    b
}
fn stack(spacing: f64) -> Ref<StackPanel> {
    let p = StackPanel::new();
    p.set_spacing(spacing);
    p
}
fn row(spacing: f64) -> Ref<StackPanel> {
    let p = stack(spacing);
    p.set_orientation(Orientation::Horizontal);
    p
}
fn grid(cols: &str, rows: &str) -> Ref<Grid> {
    let g = Grid::new();
    if !cols.is_empty() {
        g.set_column_definitions(cols.parse::<ColumnDefinitions>().unwrap());
    }
    if !rows.is_empty() {
        g.set_row_definitions(rows.parse::<RowDefinitions>().unwrap());
    }
    g
}
fn put(g: &Grid, x: i32, y: i32, c: Ref<Control>) {
    Grid::set_column(&c, x);
    Grid::set_row(&c, y);
    g.children().add(c);
}
fn center(c: &Control) {
    c.set_vertical_alignment(V::Center);
}
fn icon(name: &str, ink: &str, size: f64) -> Ref<Grid> {
    let (path, fill) = match name {
        "play" => ("M4 2 L19 11 L4 20 Z", true),
        "pause" => ("M4 2 L8 2 L8 20 L4 20 Z M14 2 L18 2 L18 20 L14 20 Z", true),
        "next" => ("M2 3 L15 11 L2 19 Z M17 3 L20 3 L20 19 L17 19 Z", true),
        "prev" => ("M20 3 L7 11 L20 19 Z M2 3 L5 3 L5 19 L2 19 Z", true),
        "home" => ("M2 10 L11 2 L20 10 M4 9 V20 H9 V14 H13 V20 H18 V9", false),
        "search" => ("M15 9 A6 6 0 1 1 3 9 A6 6 0 1 1 15 9 M14 14 L20 20", false),
        "library" => ("M4 3 V19 M9 3 V19 M13 4 L19 18", false),
        "heart" => ("M11 19 L3.5 12 C-2 6 5 1 11 7 C17 1 24 6 18.5 12 Z", false),
        "heart-on" => ("M11 19 L3.5 12 C-2 6 5 1 11 7 C17 1 24 6 18.5 12 Z", true),
        "queue" => ("M3 4 H19 M3 10 H19 M3 16 H13 M17 14 L21 17 L17 20 Z", false),
        "devices" => ("M2 3 H20 V14 H2 Z M11 14 V19 M6 19 H16", false),
        "shuffle" => (
            "M2 5 H5 C10 5 11 17 16 17 H20 M16 13 L20 17 L16 21 M2 17 H5 C10 17 11 5 16 5 H20 M16 1 L20 5 L16 9",
            false,
        ),
        "repeat" => (
            "M18 7 H5 L2 10 M14 3 L18 7 L14 11 M4 15 H17 L20 12 M8 11 L4 15 L8 19",
            false,
        ),
        "volume" => (
            "M2 8 H6 L11 3 V19 L6 14 H2 Z M15 7 Q19 11 15 15 M18 4 Q25 11 18 18",
            false,
        ),
        "pin" => ("M6 3 H16 M8 3 V9 L5 13 H17 L14 9 V3 M11 13 V21", false),
        "plus" => ("M11 3 V19 M3 11 H19", false),
        "arrow" => ("M3 11 H19 M13 5 L19 11 L13 17", false),
        "external" => ("M12 3 H20 V11 M20 3 L9 14 M8 5 H3 V20 H18 V15", false),
        "music" => (
            "M8 16 V5 L19 3 V13 M8 16 A3 2 0 1 1 2 16 A3 2 0 1 1 8 16 M19 13 A3 2 0 1 1 13 13 A3 2 0 1 1 19 13",
            false,
        ),
        "logo" => ("M2 13 L5 9 L8 15 L11 4 L14 18 L17 8 L20 12", false),
        _ => ("M3 11 H19", false),
    };
    let slot = grid("", "");
    slot.set_width(size);
    slot.set_height(size);
    center(&slot);
    let Ok(geometry) = Geometry::parse(path) else {
        return slot;
    };
    let bounds = geometry.bounds();
    let scale = size / bounds.width.max(bounds.height);
    // Uniform stretch positions the path at the top left of its layout bounds.
    // Fit the path to its own aspect ratio, then center it in the square slot.
    let p = Path::new();
    p.set_data(Some(geometry));
    p.set_width(bounds.width * scale);
    p.set_height(bounds.height * scale);
    p.set_stretch(Stretch::Uniform);
    p.set_horizontal_alignment(H::Center);
    center(&p);
    if fill {
        p.set_fill(Some(brush(ink)));
    } else {
        p.set_stroke(Some(brush(ink)));
        p.set_stroke_thickness(1.65);
    }
    put(&slot, 0, 0, p.upcast());
    slot
}
fn button(name: &str, content: Ref<Control>, primary: bool, f: impl Fn() + 'static) -> Ref<Button> {
    let b = Button::new();
    b.set_content(Some(Control::boxed(content)));
    b.set_padding(Thickness::new(10.0, 6.0, 10.0, 6.0));
    b.set_corner_radius(CornerRadius::uniform(0.0));
    b.set_background(Some(brush(if primary { ACCENT } else { "Transparent" })));
    if !primary && LIGHT_PAGE.get() {
        for (key, ink) in [
            ("ButtonBackgroundPointerOver", "#EFE5EE"),
            ("ButtonBackgroundPressed", "#F3D6E6"),
        ] {
            b.resources().set(key, Some(Rc::new(brush(ink))));
        }
    }
    if primary {
        b.resources().set(
            "ButtonBackgroundPointerOver",
            Some(Rc::new(brush("#FF85B7"))),
        );
        b.resources()
            .set("ButtonBackgroundPressed", Some(Rc::new(brush("#D51F6F"))));
    }
    b.set_border_thickness(Thickness::uniform(0.0));
    b.set_horizontal_content_alignment(H::Center);
    b.set_vertical_content_alignment(V::Center);
    AutomationProperties::set_name(&b, Some(name));
    b.click(move |_, _| f());
    b
}
fn icon_button(
    name: &str,
    glyph: &str,
    ink: &str,
    size: f64,
    f: impl Fn() + 'static,
) -> Ref<Button> {
    let b = button(name, icon(glyph, ink, size).upcast(), false, f);
    b.set_horizontal_alignment(H::Center);
    center(&b);
    b.set_padding(Thickness::uniform(0.0));
    b
}
fn action(name: &str, glyph: &str, ink: &str, code: &'static str, arg: &str) -> Ref<Button> {
    let arg = arg.to_owned();
    let b = icon_button(name, glyph, ink, 16.0, move || send(code, &arg));
    b.set_width(35.0);
    b.set_height(34.0);
    b
}
fn pill(name: &str, glyph: &str, primary: bool, code: &'static str, arg: &str) -> Ref<Button> {
    let ink = if primary { "#220E19" } else { TEXT };
    let r = row(8.0);
    r.children().add(icon(glyph, ink, 14.0).upcast::<Control>());
    let t = strong(name, 12.0, ink);
    center(&t);
    r.children().add(t.upcast::<Control>());
    let arg = arg.to_owned();
    let b = button(name, r.upcast(), primary, move || send(code, &arg));
    b.set_height(36.0);
    b.set_padding(Thickness::new(14.0, 0.0, 14.0, 0.0));
    if !primary {
        b.set_background(Some(brush(PANEL)));
    }
    b
}
fn reveal(b: &Ref<Border>) {
    let light = LIGHT_PAGE.get();
    let fill = RevealBackgroundBrush::new();
    fill.set_color(color(if light { "#09FF3595" } else { "#06FF3595" }));
    fill.set_fallback_color(color("Transparent"));
    fill.set_target_theme(ApplicationTheme::Dark);
    let edge = RevealBorderBrush::new();
    edge.set_color(color(if light { "#22FF3595" } else { "#1CFF3595" }));
    edge.set_fallback_color(color("Transparent"));
    edge.set_target_theme(ApplicationTheme::Dark);
    b.set_background(Some(fill.into()));
    b.set_border_brush(Some(edge.into()));
    b.set_border_thickness(Thickness::uniform(1.0));
    macro_rules! follow {
        ($event:expr, $state:expr) => {{
            let weak = b.downgrade();
            b.add_handler($event, move |_, _| {
                if let Some(b) = weak.upgrade() {
                    RevealBrush::set_state(&b, $state);
                }
            });
        }};
    }
    follow!(
        InputElement::pointer_entered_event(),
        RevealBrushState::PointerOver
    );
    follow!(
        InputElement::pointer_exited_event(),
        RevealBrushState::Normal
    );
    follow!(
        InputElement::pointer_pressed_event(),
        RevealBrushState::Pressed
    );
    follow!(
        InputElement::pointer_released_event(),
        RevealBrushState::PointerOver
    );
}
fn cover(url: &str, height: f64, radius: f64) -> Ref<Border> {
    let external = url.starts_with("https://");
    let b = surface("#2A202B", 0.0, if external { 0.0 } else { radius });
    b.set_height(height);
    b.set_clip_to_bounds(true);
    if url.is_empty() {
        let i = icon("music", ACCENT, height.min(54.0) * 0.46);
        i.set_horizontal_alignment(H::Center);
        b.set_child(i);
        return b;
    }
    let image = Image::new();
    image.set_stretch(if external {
        Stretch::Uniform
    } else {
        Stretch::UniformToFill
    });
    image.set_is_hit_test_visible(false);
    b.set_child(image.clone());
    let need = UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        if let Some(bitmap) = ui.art.get(url) {
            image.set_source(Some(bitmap.clone()));
            false
        } else {
            let pending = ui.waiting.entry(url.to_owned()).or_default();
            pending.retain(|image| image.upgrade().is_some());
            pending.push(image.downgrade());
            pending.len() == 1
        }
    });
    if need {
        send("art", url);
    }
    b
}
fn scroll() -> Ref<ScrollViewer> {
    let s = ScrollViewer::new();
    s.set_horizontal_scroll_bar_visibility(ScrollBarVisibility::Disabled);
    s.set_vertical_scroll_bar_visibility(ScrollBarVisibility::Auto);
    s.set_template(Some(FuncControlTemplate::for_type::<ScrollViewer>(
        |viewer, scope| {
            let layout = grid("*,Auto", "");
            layout.set_background(viewer.background());

            let presenter = ScrollContentPresenter::new();
            presenter.set_name(Some("PART_ContentPresenter".into()));
            presenter.set_background(Some(brush("Transparent")));
            presenter.set_clip_to_bounds(true);
            presenter.bind_binding(
                ContentPresenter::padding_property().as_property(),
                &TemplateBinding::new(TemplatedControl::padding_property().as_property()),
            );
            presenter.bind_binding(
                ContentPresenter::content_template_property().as_property(),
                &TemplateBinding::new(ContentControl::content_template_property().as_property()),
            );
            let presenter = presenter.register_in_name_scope(&**scope);
            let gesture = ScrollGestureRecognizer::new();
            gesture.set_can_vertically_scroll(true);
            gesture.set_is_scroll_inertia_enabled(viewer.is_scroll_inertia_enabled());
            for (property, path) in [
                (
                    ScrollGestureRecognizer::offset_property().as_property(),
                    "Offset",
                ),
                (
                    ScrollGestureRecognizer::viewport_property().as_property(),
                    "Viewport",
                ),
                (
                    ScrollGestureRecognizer::extent_property().as_property(),
                    "Extent",
                ),
            ] {
                let binding = ReflectionBinding::new(path);
                binding.set_element_name(Some("PART_ContentPresenter".into()));
                binding.set_name_scope(Some(Rc::downgrade(&scope.0)));
                gesture.bind_binding(property, &binding);
            }
            presenter.gesture_recognizers().add(gesture);
            put(&layout, 0, 0, presenter.upcast());

            let bar = ScrollBar::new();
            bar.set_name(Some("PART_VerticalScrollBar".into()));
            bar.set_orientation(Orientation::Vertical);
            bar.set_is_tab_stop(false);
            bar.set_min_width(0.0);
            bar.set_width(10.0);
            bar.set_margin(Thickness::new(6.0, 0.0, 0.0, 0.0));
            bar.set_template(Some(FuncControlTemplate::for_type::<ScrollBar>(
                |_, scope| {
                    let track = Track::new();
                    track.set_orientation(Orientation::Vertical);
                    track.set_is_direction_reversed(true);
                    for (target, source, mode) in [
                        (
                            Track::minimum_property().as_property(),
                            RangeBase::minimum_property().as_property(),
                            BindingMode::OneWay,
                        ),
                        (
                            Track::maximum_property().as_property(),
                            RangeBase::maximum_property().as_property(),
                            BindingMode::OneWay,
                        ),
                        (
                            Track::value_property().as_property(),
                            RangeBase::value_property().as_property(),
                            BindingMode::TwoWay,
                        ),
                        (
                            Track::viewport_size_property().as_property(),
                            ScrollBar::viewport_size_property().as_property(),
                            BindingMode::OneWay,
                        ),
                    ] {
                        track.bind_binding(target, &TemplateBinding::new(source).with_mode(mode));
                    }

                    let thumb = Thumb::new();
                    thumb.set_name(Some("VerticalThumb".into()));
                    thumb.set_min_height(16.0);
                    thumb.set_template(Some(FuncControlTemplate::new(|_, _| {
                        surface("#B3B3B3", 0.0, 0.0).upcast()
                    })));
                    track.set_thumb(thumb.register_in_name_scope(&**scope));
                    track.set_decrease_button(
                        scroll_repeat("PART_PageUpButton", None).register_in_name_scope(&**scope),
                    );
                    track.set_increase_button(
                        scroll_repeat("PART_PageDownButton", None).register_in_name_scope(&**scope),
                    );

                    let rail = surface("Transparent", 0.0, 0.0);
                    rail.set_border_brush(Some(brush("#A6A6A6")));
                    rail.set_border_thickness(Thickness::new(0.0, 0.0, 1.0, 0.0));
                    let controls = grid("", "11,*,11");
                    put(
                        &controls,
                        0,
                        0,
                        scroll_repeat("PART_LineUpButton", Some(true))
                            .register_in_name_scope(&**scope)
                            .upcast(),
                    );
                    put(&controls, 0, 1, track.upcast());
                    put(
                        &controls,
                        0,
                        2,
                        scroll_repeat("PART_LineDownButton", Some(false))
                            .register_in_name_scope(&**scope)
                            .upcast(),
                    );
                    rail.set_child(controls);
                    rail.upcast()
                },
            )));
            put(&layout, 1, 0, bar.register_in_name_scope(&**scope).upcast());
            layout.upcast()
        },
    )));
    s
}
fn scroll_repeat(name: &str, arrow_up: Option<bool>) -> Ref<RepeatButton> {
    let button = RepeatButton::new();
    button.set_name(Some(name.into()));
    button.set_focusable(false);
    button.set_min_width(0.0);
    button.set_min_height(0.0);
    button.set_horizontal_alignment(H::Stretch);
    button.set_vertical_alignment(V::Stretch);
    button.set_template(Some(FuncControlTemplate::new(move |_, _| {
        let hit_area = surface("Transparent", 0.0, 0.0);
        if let Some(up) = arrow_up {
            let arrow = Path::new();
            arrow.set_data(
                Geometry::parse(if up {
                    "M0 3 L2.5 0 L5 3 Z"
                } else {
                    "M0 0 L5 0 L2.5 3 Z"
                })
                .ok(),
            );
            arrow.set_width(5.0);
            arrow.set_height(3.0);
            arrow.set_stretch(Stretch::Uniform);
            arrow.set_fill(Some(brush("#858585")));
            arrow.set_horizontal_alignment(H::Center);
            center(&arrow);
            hit_area.set_child(arrow);
        }
        hit_area.upcast()
    })));
    button
}
fn add_reveal_light(root: &Grid, rows: i32) -> WeakRef<Border> {
    // Paint over the page backgrounds and clip at the app's outer edge.
    let lighting = Border::new();
    lighting.set_is_hit_test_visible(false);
    Grid::set_row_span(&lighting, rows);
    put(root, 0, 0, lighting.clone().upcast());
    effects::reveal_light(root, &lighting)
}

fn build() {
    let (width, host, model) = UI.with(|s| {
        let s = s.borrow();
        (s.width, s.shell.clone().unwrap(), s.model.clone())
    });
    let mobile = width < 790.0;
    let root = grid("", if mobile { "99,*,77" } else { "76,*,86" });
    root.set_clip_to_bounds(true);

    let main = scroll();
    let body = surface(BG, 0.0, 0.0);
    body.set_clip_to_bounds(true);
    body.size_changed(|_, event| {
        if event.height_changed() {
            let height = event.new_size().height;
            use ferroui_base::threading::{Dispatcher, DispatcherPriority};
            Dispatcher::ui_thread().post_local(
                move || {
                    let stage =
                        UI.with(|ui| ui.borrow().now_stage.as_ref().and_then(WeakRef::upgrade));
                    if let Some(stage) = stage {
                        stage.set_height(height.max(340.0));
                    }
                },
                DispatcherPriority::NORMAL,
            );
        }
    });
    let chrome = surface(BG, 0.0, 0.0);
    chrome.set_child(top_bar(mobile, &model));
    put(&root, 0, 0, chrome.clone().upcast());
    put(&root, 0, 1, body.clone().upcast());
    let footer = surface(BG, 0.0, 0.0);
    let backdrop = surface("#01000000", 0.0, 0.0);
    backdrop.set_is_hit_test_visible(false);
    backdrop.set_vertical_alignment(V::Bottom);
    backdrop.set_height(62.0);
    let transport = surface(BG, 0.0, 0.0);
    let layers = grid("", "");
    put(&layers, 0, 0, backdrop.clone().upcast());
    put(&layers, 0, 0, footer.clone().upcast());
    footer.set_background(None);
    transport.set_child(layers);
    put(&root, 0, 2, transport.clone().upcast());

    #[cfg(target_os = "emscripten")]
    let reveal = (!UI.with(|ui| ui.borrow().reduced)).then(|| add_reveal_light(&root, 3));

    UI.with(|s| {
        let mut s = s.borrow_mut();
        #[cfg(target_os = "emscripten")]
        {
            s.reveal = reveal;
        }
        s.fire = Some(effects::TransportFire::new(&backdrop, s.reduced));
        s.body = Some(body);
        s.main = Some(main);
        s.collection = None;
        s.footer = Some(footer);
        s.chrome = Some(chrome);
        s.transport = Some(transport);
    });
    host.set_child(root);
    refresh(true, true, true);
}

fn top_bar(mobile: bool, model: &Model) -> Ref<Control> {
    let top = grid("7,*", "");
    let stripe = surface(ACCENT, 0.0, 0.0);
    let fill = LinearGradientBrush::new();
    fill.set_end_point(RelativePoint::new(0.0, 1.0, RelativeUnit::Relative));
    fill.gradient_stops()
        .add(GradientStop::with_color_and_offset(color("#F00B95"), 0.0));
    fill.gradient_stops()
        .add(GradientStop::with_color_and_offset(color("#FF6F20"), 1.0));
    stripe.set_background(Some(fill.into()));
    stripe.set_vertical_alignment(V::Center);
    stripe.set_height(50.0);
    put(&top, 0, 0, stripe.upcast());

    let g = grid(
        if mobile { "*,Auto" } else { "Auto,*,Auto" },
        if mobile { "49,42" } else { "" },
    );
    g.set_margin(Thickness::new(
        if mobile { 7.0 } else { 20.0 },
        0.0,
        15.0,
        0.0,
    ));
    let navigation = row(if mobile { 0.0 } else { 5.0 });
    center(&navigation);
    let mut labels = Vec::new();
    for (title, accessible, route) in [
        ("QUICKPLAY", "Quickplay", "home"),
        ("COLLECTION", "Your library", "library"),
        ("NOW PLAYING", "Now playing", "queue"),
    ] {
        let text = strong(title, if mobile { 10.0 } else { 12.0 }, MUTED);
        text.set_letter_spacing(if mobile { 0.05 } else { 0.6 });
        labels.push((route, text.clone()));
        let tab = button(accessible, text.upcast(), false, move || {
            send("navigate", route)
        });
        tab.set_padding(Thickness::new(
            if mobile { 6.0 } else { 10.0 },
            9.0,
            if mobile { 6.0 } else { 10.0 },
            9.0,
        ));
        navigation.children().add(tab.upcast::<Control>());
    }
    put(&g, 0, 0, navigation.upcast());

    let search = TextBox::new();
    search.set_min_width(0.0);
    search.set_max_width(if mobile { 560.0 } else { 285.0 });
    search.set_height(32.0);
    search.set_placeholder_text(Some("Search music"));
    search.set_text(Some(&model.query));
    search.set_background(Some(brush("#211923")));
    search.set_corner_radius(CornerRadius::uniform(0.0));
    search.set_inner_left_content(Some(Control::boxed(icon("search", MUTED, 13.0))));
    center(&search);
    AutomationProperties::set_name(&search, Some("Search songs, artists, albums"));
    let weak = search.downgrade();
    search.text_changed(move |_, _| {
        if let Some(search) = weak.upgrade() {
            send("query", &search.text().unwrap_or_default());
        }
    });
    if mobile {
        Grid::set_column_span(&search, 2);
        put(&g, 0, 1, search.clone().upcast());
    } else {
        put(&g, 1, 0, search.clone().upcast());
    }
    UI.with(|ui| ui.borrow_mut().account_label = None);
    let tools = row(5.0);
    center(&tools);
    if !mobile {
        tools.children().add(
            action("Playback devices", "devices", MUTED, "navigate", "devices").upcast::<Control>(),
        );
    }
    tools.children().add(if mobile {
        action("Spotify settings", "logo", ACCENT, "settings", "").upcast::<Control>()
    } else {
        let name = if model.connected {
            "Account"
        } else {
            "Connect Spotify"
        };
        let label = strong(name, 12.0, TEXT);
        UI.with(|ui| ui.borrow_mut().account_label = Some(label.clone()));
        button(name, label.upcast(), false, || send("settings", "")).upcast::<Control>()
    });
    put(&g, if mobile { 1 } else { 2 }, 0, tools.upcast());
    put(&top, 1, 0, g.upcast());
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        ui.search = Some(search);
        ui.nav = labels;
    });
    top.upcast()
}
fn header(title: &str, detail: &str) -> Ref<Control> {
    let g = grid("*,Auto", "");
    put(&g, 0, 0, strong(title, 23.0, TEXT).upcast());
    let t = kicker(detail, DIM);
    center(&t);
    put(&g, 1, 0, t.upcast());
    g.upcast()
}
fn artwork_tile(item: &Item, size: f64, play: bool) -> Ref<Button> {
    let art = cover(&item.art, size, 0.0);
    art.set_width(size);
    let id = item.id.clone();
    let name = if play {
        format!("Play {}", item.title)
    } else {
        item.title.clone()
    };
    let tile = button(&name, art.upcast(), false, move || {
        send(if play { "play" } else { "open" }, &id)
    });
    tile.set_padding(Thickness::uniform(0.0));
    tile.set_width(size);
    tile.set_height(size);
    ToolTip::set_tip(&tile, Some(Rc::new(item.title.clone())));
    tile
}

fn quickplay(m: &Model, width: f64, animate: bool) -> Ref<Control> {
    let mobile = width < 790.0;
    let tiles = if width >= 980.0 {
        96.0
    } else if mobile {
        62.0
    } else {
        73.0
    };
    let scene = grid("", "");
    let wash = surface(BG, 0.0, 0.0);
    wash.set_height(if mobile { 360.0 } else { 380.0 });
    wash.set_max_width(1100.0);
    wash.set_vertical_alignment(V::Top);
    wash.set_is_hit_test_visible(false);
    effects::light(&wash);
    put(&scene, 0, 0, wash.upcast());

    let content = stack(if mobile { 22.0 } else { 44.0 });
    content.set_max_width(1220.0);
    content.set_margin(Thickness::new(
        if mobile { 18.0 } else { 36.0 },
        if mobile { 10.0 } else { 58.0 },
        if mobile { 18.0 } else { 36.0 },
        28.0,
    ));
    let sections = grid(
        if mobile { "" } else { "1*,2.15*,1.7*" },
        if mobile { "Auto,Auto,Auto" } else { "" },
    );
    let pins = stack(8.0);
    pins.children()
        .add(metro_title("pins", 42.0).upcast::<Control>());
    if m.pins.is_empty() {
        let note = label("Pin a playlist from your collection.", 12.0, MUTED);
        note.set_text_wrapping(TextWrapping::Wrap);
        pins.children().add(note.upcast::<Control>());
        let b = pill("Your collection", "arrow", false, "navigate", "library");
        b.set_horizontal_alignment(H::Left);
        pins.children().add(b.upcast::<Control>());
    } else {
        pins.children().add(tile_rows(&m.pins, 2, tiles * 0.74));
    }
    let new_music = stack(8.0);
    new_music
        .children()
        .add(metro_title("new", 42.0).upcast::<Control>());
    if let Some(featured) = m.collections.first() {
        let mosaic = row(5.0);
        mosaic
            .children()
            .add(artwork_tile(featured, tiles * 1.35, false).upcast::<Control>());
        mosaic.children().add(tile_rows(
            &m.collections[1..m.collections.len().min(5)],
            2,
            tiles * 0.65,
        ));
        new_music.children().add(mosaic.upcast::<Control>());
        let play = pill("Play mix", "play", true, "play", &featured.id);
        play.set_horizontal_alignment(H::Left);
        new_music.children().add(play.upcast::<Control>());
    }
    if m.collections.is_empty() {
        new_music.children().add(
            label("Music from your collection appears here.", 12.0, MUTED).upcast::<Control>(),
        );
    }
    let history = stack(8.0);
    history
        .children()
        .add(metro_title("history", 42.0).upcast::<Control>());
    if m.recent.is_empty() {
        let note = label("Recently played music appears here.", 12.0, MUTED);
        note.set_text_wrapping(TextWrapping::Wrap);
        history.children().add(note.upcast::<Control>());
    } else {
        history.children().add(tile_rows(
            &m.recent,
            if mobile { 4 } else { 3 },
            tiles * 0.73,
        ));
    }
    for (index, section) in [pins, new_music, history].into_iter().enumerate() {
        let frame = surface("Transparent", 0.0, 0.0);
        frame.set_margin(Thickness::new(
            0.0,
            0.0,
            9.0,
            if mobile { 16.0 } else { 0.0 },
        ));
        frame.set_child(section);
        frame.set_horizontal_alignment(H::Left);
        if animate {
            effects::entrance(&frame, 26.0, index as u64 * 38);
        }
        put(
            &sections,
            if mobile { 0 } else { index as i32 },
            if mobile { index as i32 } else { 0 },
            frame.upcast(),
        );
    }
    content.children().add(sections.upcast::<Control>());
    if !m.connected {
        let mixes = stack(8.0);
        mixes.set_horizontal_alignment(if mobile { H::Left } else { H::Center });
        mixes
            .children()
            .add(metro_title("mixes", 28.0).upcast::<Control>());
        let line = row(5.0);
        for mix in m.collections.iter().take(4) {
            let title = format!("Play {}", mix.title);
            let id = mix.id.clone();
            let art = cover(&mix.art, tiles * 0.68, 0.0);
            art.set_width(tiles * 0.68);
            let tile = button(&title, art.upcast(), false, move || send("play", &id));
            tile.set_padding(Thickness::uniform(0.0));
            line.children().add(tile.upcast::<Control>());
        }
        mixes.children().add(line.upcast::<Control>());
        content.children().add(mixes.upcast::<Control>());
    } else if m.collections.is_empty() && !m.items.is_empty() {
        let tracks = stack(3.0);
        tracks
            .children()
            .add(metro_title("liked songs", 30.0).upcast::<Control>());
        for (index, item) in m.items.iter().take(6).enumerate() {
            tracks.children().add(track(item, index, false));
        }
        content.children().add(tracks.upcast::<Control>());
    }
    if !m.status.is_empty() {
        let message = label(&m.status, 13.0, ACCENT);
        message.set_text_wrapping(TextWrapping::Wrap);
        content.children().add(message.upcast::<Control>());
    }
    put(&scene, 0, 0, content.upcast());
    scene.upcast()
}

fn tile_rows(items: &[Item], columns: usize, size: f64) -> Ref<Control> {
    let rows = stack(5.0);
    for chunk in items.chunks(columns) {
        let line = row(5.0);
        for item in chunk {
            line.children()
                .add(artwork_tile(item, size, item.kind == "track").upcast::<Control>());
        }
        rows.children().add(line.upcast::<Control>());
    }
    rows.upcast()
}

fn virtual_list<T: PartialEq + 'static>(
    items: impl IntoIterator<Item = T>,
    spacing: f64,
    build: impl Fn(&T) -> Ref<Control> + 'static,
) -> Ref<Control> {
    let repeater = virtual_repeater(spacing, build);
    set_repeater_items(&repeater, items);
    repeater.upcast()
}

fn virtual_repeater<T: PartialEq + 'static>(
    spacing: f64,
    build: impl Fn(&T) -> Ref<Control> + 'static,
) -> Ref<ItemsRepeater> {
    let light = LIGHT_PAGE.get();
    let repeater = ItemsRepeater::new();
    let layout = StackLayout::new();
    layout.set_spacing(spacing);
    repeater.set_layout(Some(layout.upcast()));
    let template = FuncDataTemplate::for_type::<T>(
        move |item, _| {
            let previous = LIGHT_PAGE.replace(light);
            let control = build(item);
            LIGHT_PAGE.set(previous);
            Some(control)
        },
        false,
    );
    repeater.set_data_template(Some(template));
    repeater
}

fn set_repeater_items<T: PartialEq + 'static>(
    repeater: &ItemsRepeater,
    items: impl IntoIterator<Item = T>,
) {
    repeater.set_items_source(Some(ItemsSource::from_items(
        items
            .into_iter()
            .map(|item| Some(Rc::new(item) as BoxedValue)),
    )));
}

fn track_list<'a>(items: impl IntoIterator<Item = &'a Item>, compact: bool) -> Ref<Control> {
    virtual_list(
        items.into_iter().cloned().enumerate(),
        2.0,
        move |(index, item)| track(item, *index, compact),
    )
}

fn collection_cards(items: &[Item], cols: usize) -> Ref<Control> {
    virtual_list(
        items.chunks(cols).map(<[Item]>::to_vec),
        8.0,
        move |chunk| collection_card_row(chunk, cols),
    )
}

fn collection_card_row(chunk: &[Item], cols: usize) -> Ref<Control> {
    let g = grid(&vec!["*"; cols].join(","), "");
    for (idx, item) in chunk.iter().enumerate() {
        let card = surface("Transparent", 0.0, 0.0);
        card.set_margin(Thickness::new(
            if idx == 0 { 0.0 } else { 4.0 },
            0.0,
            if idx + 1 == cols { 0.0 } else { 4.0 },
            0.0,
        ));
        let light = surface("Transparent", 0.0, 0.0);
        reveal(&light);
        let p = stack(5.0);
        let size = if cols > 2 { 104.0 } else { 112.0 };
        let image = cover(&item.art, size, 0.0);
        image.set_width(size);
        image.set_horizontal_alignment(H::Left);
        p.children().add(image.upcast::<Control>());
        p.children()
            .add(strong(&item.title, 13.0, TEXT).upcast::<Control>());
        p.children()
            .add(label(&item.artist, 10.5, MUTED).upcast::<Control>());
        let id = item.id.clone();
        let b = button(&item.title, p.upcast(), false, move || send("open", &id));
        b.set_horizontal_alignment(H::Stretch);
        b.set_horizontal_content_alignment(H::Stretch);
        b.set_padding(Thickness::uniform(8.0));
        light.set_child(b);
        card.set_child(light);
        put(&g, idx as i32, 0, card.upcast());
    }
    g.upcast()
}
fn track(item: &Item, index: usize, compact: bool) -> Ref<Control> {
    let light = surface("Transparent", 0.0, 0.0);
    reveal(&light);
    let g = grid(
        if compact {
            "*,35"
        } else if item.id.starts_with("spotify:") {
            "*,35,35,35,42"
        } else {
            "*,35,35,48"
        },
        "",
    );
    let r = grid(if compact { "33,*" } else { "25,38,*" }, "");
    let col = if compact { 0 } else { 1 };
    if !compact {
        let n = label(&format!("{:02}", index + 1), 10.0, DIM);
        center(&n);
        put(&r, 0, 0, n.upcast());
    }
    let image = cover(&item.art, if compact { 30.0 } else { 35.0 }, 5.0);
    image.set_width(if compact { 30.0 } else { 35.0 });
    put(&r, col, 0, image.upcast());
    let names = stack(1.0);
    center(&names);
    names.set_margin(Thickness::new(7.0, 0.0, 0.0, 0.0));
    names
        .children()
        .add(strong(&item.title, 12.0, TEXT).upcast::<Control>());
    names
        .children()
        .add(label(&item.artist, 10.5, MUTED).upcast::<Control>());
    put(&r, col + 1, 0, names.upcast());
    let id = item.id.clone();
    let b = button(
        &format!("Play {}", item.title),
        r.upcast(),
        false,
        move || send("play", &id),
    );
    b.set_horizontal_alignment(H::Stretch);
    b.set_horizontal_content_alignment(H::Stretch);
    b.set_padding(Thickness::new(6.0, 4.0, 4.0, 4.0));
    put(&g, 0, 0, b.upcast());
    if compact {
        let t = label(&time(item.duration), 10.0, DIM);
        center(&t);
        put(&g, 1, 0, t.upcast());
    } else {
        put(
            &g,
            1,
            0,
            action(
                if item.liked {
                    "Remove from liked songs"
                } else {
                    "Save to liked songs"
                },
                if item.liked { "heart-on" } else { "heart" },
                if item.liked { ACCENT } else { DIM },
                "like",
                &item.id,
            )
            .upcast(),
        );
        put(
            &g,
            2,
            0,
            action("Add to queue", "plus", MUTED, "enqueue", &item.id).upcast(),
        );
        let t = label(&time(item.duration), 11.0, DIM);
        center(&t);
        t.set_horizontal_alignment(H::Center);
        let col = if item.id.starts_with("spotify:") {
            put(
                &g,
                3,
                0,
                action(
                    "Open track in Spotify",
                    "external",
                    MUTED,
                    "spotify-link",
                    &item.id,
                )
                .upcast(),
            );
            4
        } else {
            3
        };
        put(&g, col, 0, t.upcast());
    }
    light.set_child(g);
    light.upcast()
}
fn main_page(m: &Model, width: f64, animate: bool) -> Ref<Control> {
    if m.page == "home" {
        return quickplay(m, width, animate);
    }
    if m.page == "queue" {
        return now_playing(m, width, animate);
    }
    let mobile = width < 790.0;
    let page = stack(if mobile { 12.0 } else { 15.0 });
    page.set_margin(Thickness::uniform(if mobile { 12.0 } else { 20.0 }));
    #[cfg(not(target_os = "emscripten"))]
    if m.page == "account" {
        page.children().add(header("Spotify account", ""));
        let explanation = if m.connected {
            "Spotify is connected. Choose a Spotify Connect device to listen."
        } else if m.connecting {
            "Finish signing in on Spotify in your browser, then return to this window."
        } else {
            "Sign in on Spotify to access your library and play on a Spotify Connect device."
        };
        let text = label(explanation, 14.0, MUTED);
        text.set_text_wrapping(TextWrapping::Wrap);
        page.children().add(text.upcast::<Control>());
        if !m.status.is_empty() {
            let text = label(&m.status, 13.0, ACCENT);
            text.set_text_wrapping(TextWrapping::Wrap);
            page.children().add(text.upcast::<Control>());
        }
        if m.connected {
            page.children().add(
                pill("Playback devices", "devices", true, "navigate", "devices")
                    .upcast::<Control>(),
            );
            page.children()
                .add(pill("Sign out", "external", false, "disconnect", "").upcast::<Control>());
        } else if m.connecting {
            page.children().add(
                pill("Cancel sign-in", "external", false, "disconnect", "").upcast::<Control>(),
            );
        } else if let Some(client) = crate::desktop::configured_client() {
            page.children().add(
                pill("Sign in with Spotify", "external", true, "connect", &client)
                    .upcast::<Control>(),
            );
        } else {
            let input = TextBox::new();
            input.set_max_width(460.0);
            input.set_horizontal_alignment(H::Left);
            input.set_placeholder_text(Some("Spotify developer Client ID"));
            AutomationProperties::set_name(&input, Some("Spotify developer Client ID"));
            input.set_text(Some(&m.client_id));
            let help = label(crate::desktop::REDIRECT, 13.0, MUTED);
            page.children().add(
                label(
                    "Register this redirect URL in your Spotify developer app",
                    13.0,
                    MUTED,
                )
                .upcast::<Control>(),
            );
            page.children().add(help.upcast::<Control>());
            page.children().add(input.clone().upcast::<Control>());
            let content = strong("Sign in with Spotify", 12.0, "#220E19");
            let sign_in = button("Sign in with Spotify", content.upcast(), true, move || {
                send("connect", &input.text().unwrap_or_default());
            });
            page.children().add(sign_in.upcast::<Control>());
            page.children().add(
                pill(
                    "Spotify developer dashboard",
                    "external",
                    false,
                    "dashboard",
                    "",
                )
                .upcast::<Control>(),
            );
        }
        return page.upcast();
    }
    let cols = if width >= 980.0 {
        4
    } else if width >= 600.0 {
        3
    } else {
        2
    };
    {
        page.children().add(header(&m.title, ""));
        if !m.caption.is_empty() {
            let t = label(&m.caption, 13.0, MUTED);
            t.set_text_wrapping(TextWrapping::Wrap);
            page.children().add(t.upcast::<Control>());
        }
        if m.page == "devices" {
            if !m.status.is_empty() {
                let t = label(&m.status, 12.0, ACCENT);
                t.set_text_wrapping(TextWrapping::Wrap);
                page.children().add(t.upcast::<Control>());
            }
            page.children()
                .add(pill("Refresh devices", "devices", false, "devices", "").upcast::<Control>());
            for device in &m.items {
                page.children().add(
                    pill(&device.title, "devices", false, "transfer", &device.id)
                        .upcast::<Control>(),
                );
            }
            return page.upcast();
        }
        if m.page == "detail" {
            let actions = row(8.0);
            let has_tracks = m.items.iter().any(|i| i.kind == "track");
            if has_tracks {
                actions
                    .children()
                    .add(pill("Play all", "play", true, "play", "").upcast::<Control>());
            }
            if m.detail.starts_with("spotify:") {
                actions.children().add(
                    pill(
                        "Open in Spotify",
                        "external",
                        false,
                        "spotify-link",
                        &m.detail,
                    )
                    .upcast::<Control>(),
                );
            }
            if !m.detail.starts_with("section:") {
                let name = if m.pins.iter().any(|item| item.id == m.detail) {
                    "Unpin from Quickplay"
                } else {
                    "Pin to Quickplay"
                };
                actions
                    .children()
                    .add(pill(name, "pin", false, "pin", &m.detail).upcast::<Control>());
            }
            page.children().add(actions.upcast::<Control>());
        }
    }
    if !m.status.is_empty() {
        let t = label(&m.status, 12.0, ACCENT);
        t.set_text_wrapping(TextWrapping::Wrap);
        page.children().add(t.upcast::<Control>());
    }
    let cards = m
        .items
        .iter()
        .filter(|i| matches!(i.kind.as_str(), "album" | "artist" | "playlist"))
        .cloned()
        .collect::<Vec<_>>();
    if !cards.is_empty() {
        page.children().add(collection_cards(&cards, cols));
    }
    page.children().add(track_list(
        m.items
            .iter()
            .filter(|item| item.kind.is_empty() || item.kind == "track"),
        false,
    ));
    let frame = surface(
        if LIGHT_PAGE.get() {
            "#FCFAFC"
        } else {
            "Transparent"
        },
        0.0,
        0.0,
    );
    frame.set_child(page);
    if animate {
        effects::entrance(&frame, 20.0, 0);
    }
    frame.upcast()
}
struct CollectionPane<T> {
    host: Ref<Grid>,
    viewer: Ref<ScrollViewer>,
    repeater: Ref<ItemsRepeater>,
    items: RefCell<Vec<T>>,
}

impl<T: Clone + PartialEq + 'static> CollectionPane<T> {
    fn new(title: &str, spacing: f64, build: impl Fn(&T) -> Ref<Control> + 'static) -> Self {
        let viewer = scroll();
        viewer.set_is_scroll_chaining_enabled(false);
        AutomationProperties::set_name(&viewer, Some(title));

        let repeater = virtual_repeater(spacing, build);
        viewer.set_content(Some(Rc::new(repeater.clone().upcast::<Control>())));

        let host = grid("", "Auto,*");
        let heading = kicker(title, TEXT);
        heading.set_margin(Thickness::new(0.0, 0.0, 0.0, 8.0));
        put(&host, 0, 0, heading.upcast());
        put(&host, 0, 1, viewer.clone().upcast());
        Self {
            host,
            viewer,
            repeater,
            items: RefCell::default(),
        }
    }

    fn update(&self, items: Vec<T>) {
        if *self.items.borrow() != items {
            set_repeater_items(&self.repeater, items.iter().cloned());
            *self.items.borrow_mut() = items;
        }
    }
}

struct CollectionView {
    root: Ref<Grid>,
    artists: CollectionPane<Option<String>>,
    playlists: CollectionPane<Vec<Item>>,
    songs: CollectionPane<(usize, Item)>,
    playlist_empty: Ref<TextBlock>,
    song_empty: Ref<TextBlock>,
    tabs: Vec<Ref<TextBlock>>,
    selected_artist: RefCell<Option<String>>,
    cols: usize,
}

impl CollectionView {
    fn new(m: &Model, width: f64, animate: bool) -> Self {
        let compact = width < 790.0;
        let cols = if width >= 1360.0 {
            4
        } else if width >= 1050.0 {
            3
        } else {
            2
        };
        let root = grid("", "Auto,Auto,Auto,*");
        let inset = if compact { 16.0 } else { 44.0 };
        root.set_margin(Thickness::new(inset, 16.0, inset, 16.0));
        root.set_max_width(1370.0);

        let heading = surface("Transparent", 0.0, 0.0);
        heading.set_horizontal_alignment(H::Left);
        heading.set_child(metro_title("music", if compact { 34.0 } else { 44.0 }));
        heading.set_margin(Thickness::new(
            0.0,
            0.0,
            0.0,
            if compact { 10.0 } else { 20.0 },
        ));
        if animate {
            effects::entrance(&heading, 24.0, 0);
        }
        put(&root, 0, 0, heading.upcast());

        let filters = row(3.0);
        filters
            .children()
            .add(pill("Liked songs", "heart", false, "navigate", "liked").upcast::<Control>());
        if m.connected {
            for (name, id) in [("Artists", "section:artists"), ("Albums", "section:albums")] {
                filters
                    .children()
                    .add(pill(name, "library", false, "open", id).upcast::<Control>());
            }
        }
        filters.set_margin(Thickness::new(
            0.0,
            0.0,
            0.0,
            if compact { 10.0 } else { 20.0 },
        ));
        put(&root, 0, 1, filters.upcast());

        let artists = CollectionPane::new("ARTISTS", 1.0, artist_button);
        let playlists = CollectionPane::new("PLAYLISTS", 8.0, move |chunk: &Vec<Item>| {
            collection_card_row(chunk, cols)
        });
        let songs = CollectionPane::new("SONGS", 2.0, |(index, item)| track(item, *index, false));
        let playlist_empty = collection_empty(&playlists.host, "Your playlists appear here.");
        let song_empty = collection_empty(&songs.host, "No songs to show.");

        let catalog = grid(if compact { "" } else { "1.05*,2.1*,1.8*" }, "");
        let mut tabs = Vec::new();
        let tab_row = row(8.0);
        for (index, (name, pane)) in [
            ("Artists", &artists.host),
            ("Playlists", &playlists.host),
            ("Songs", &songs.host),
        ]
        .into_iter()
        .enumerate()
        {
            if compact {
                let text = strong(name, 12.0, TEXT);
                let tab = button(
                    &format!("Show {name}"),
                    text.clone().upcast(),
                    false,
                    move || {
                        if let Some(view) = UI.with(|ui| ui.borrow().collection.clone()) {
                            view.show_pane(index);
                        }
                    },
                );
                tab_row.children().add(tab.upcast::<Control>());
                tabs.push(text);
            } else if index > 0 {
                pane.set_margin(Thickness::new(20.0, 0.0, 0.0, 0.0));
            }
            put(
                &catalog,
                if compact { 0 } else { index as i32 },
                0,
                pane.clone().upcast(),
            );
        }
        if compact {
            tab_row.set_margin(Thickness::new(0.0, 0.0, 0.0, 10.0));
            put(&root, 0, 2, tab_row.upcast());
        }
        put(&root, 0, 3, catalog.upcast());

        let view = Self {
            root,
            artists,
            playlists,
            songs,
            playlist_empty,
            song_empty,
            tabs,
            selected_artist: RefCell::default(),
            cols,
        };
        view.show_pane(1);
        view
    }

    fn show_pane(&self, selected: usize) {
        let panes = [&self.artists.host, &self.playlists.host, &self.songs.host];
        for (index, (tab, pane)) in self.tabs.iter().zip(panes).enumerate() {
            tab.set_foreground(Some(brush(if index == selected {
                "#BA1267"
            } else {
                "#635A63"
            })));
            pane.set_is_visible(index == selected);
        }
    }

    fn update(&self, m: &Model) {
        let selected = UI.with(|ui| ui.borrow().artist_filter.clone());
        let tracks = m.items.iter().filter(|item| item.kind == "track");
        let names = tracks
            .clone()
            .filter(|item| !item.artist.is_empty())
            .map(|item| item.artist.clone())
            .collect::<std::collections::BTreeSet<_>>();
        self.artists.update(
            std::iter::once(None)
                .chain(names.into_iter().map(Some))
                .collect(),
        );
        self.playlists.update(
            m.collections
                .chunks(self.cols)
                .map(<[Item]>::to_vec)
                .collect(),
        );
        self.playlist_empty.set_is_visible(m.collections.is_empty());

        self.songs.update(
            tracks
                .filter(|item| {
                    selected
                        .as_ref()
                        .is_none_or(|artist| item.artist == *artist)
                })
                .cloned()
                .enumerate()
                .collect(),
        );
        self.song_empty
            .set_is_visible(self.songs.items.borrow().is_empty());

        if *self.selected_artist.borrow() != selected {
            let previous = self.selected_artist.replace(selected.clone());
            self.mark_artist(&previous, false);
            self.mark_artist(&selected, true);
            self.songs.viewer.scroll_to_home();
        }
    }

    fn mark_artist(&self, artist: &Option<String>, selected: bool) {
        if let Ok(index) = self.artists.items.borrow().binary_search(artist)
            && let Some(control) = self.artists.repeater.try_get_element(index as i32)
            && let Ok(button) = control.downcast::<Button>()
            && let Some(text) = button.content().as_ref().and_then(Control::from_boxed)
            && let Ok(text) = text.downcast::<TextBlock>()
        {
            text.set_foreground(Some(artist_ink(selected)));
        }
    }
}

fn collection_empty(host: &Grid, message: &str) -> Ref<TextBlock> {
    let text = label(message, 13.0, MUTED);
    text.set_vertical_alignment(V::Top);
    put(host, 0, 1, text.clone().upcast());
    text
}

fn artist_ink(selected: bool) -> Rc<dyn IBrush> {
    brush(if selected { "#BA1267" } else { TEXT })
}

fn artist_button(artist: &Option<String>) -> Ref<Control> {
    let name = artist.as_deref().unwrap_or("All artists");
    let selected = UI.with(|ui| ui.borrow().artist_filter == *artist);
    let text = label(name, 13.0, TEXT);
    text.set_foreground(Some(artist_ink(selected)));
    let selection = artist.clone();
    let button = button(name, text.upcast(), false, move || {
        let changed = UI.with(|ui| {
            let mut ui = ui.borrow_mut();
            if ui.artist_filter == selection {
                return false;
            }
            ui.artist_filter = selection.clone();
            true
        });
        if changed {
            refresh(true, false, false);
        }
        if let Some(view) = UI.with(|ui| ui.borrow().collection.clone()) {
            view.show_pane(2);
        }
    });
    button.set_horizontal_content_alignment(H::Left);
    button.set_horizontal_alignment(H::Stretch);
    button.set_padding(Thickness::new(0.0, 5.0, 5.0, 5.0));
    button.upcast()
}

fn now_playing(m: &Model, width: f64, animate: bool) -> Ref<Control> {
    let mobile = width < 790.0;
    let stage = grid("", "");
    stage.set_clip_to_bounds(true);
    if mobile {
        stage.set_height(370.0);
    } else {
        let viewport = UI.with(|ui| {
            ui.borrow()
                .body
                .as_ref()
                .map_or(480.0, |body| body.bounds().height)
        });
        stage.set_height(viewport.max(340.0));
        UI.with(|ui| ui.borrow_mut().now_stage = Some(stage.downgrade()));
    }
    stage.set_background(Some(brush(BG)));

    let mut seen = std::collections::HashSet::new();
    let covers: Vec<_> = m
        .collections
        .iter()
        .chain(&m.queue)
        .chain(&m.recent)
        .chain(std::iter::once(&m.current))
        .filter(|item| !item.art.is_empty() && seen.insert(item.art.as_str()))
        .take(20)
        .collect();
    if !covers.is_empty() {
        let columns = if mobile { 4 } else { 8 };
        let wall = grid(&vec!["*"; columns].join(","), "*,*,*");
        wall.set_is_hit_test_visible(false);
        let cell_height = if mobile { 124.0 } else { 160.0 };
        for row in 0..3 {
            for column in 0..columns {
                let item = covers[(row * 3 + column) % covers.len()];
                let art = cover(&item.art, cell_height, 0.0);
                art.set_height(f64::NAN);
                art.set_margin(Thickness::uniform(1.0));
                put(&wall, column as i32, row as i32, art.upcast());
            }
        }
        put(&stage, 0, 0, wall.upcast());
        let tint = surface(BG, 0.0, 0.0);
        let brush = LinearGradientBrush::new();
        brush.set_end_point(RelativePoint::new(1.0, 0.35, RelativeUnit::Relative));
        for (ink, offset) in [("#BD341D16", 0.0), ("#C8150E18", 0.56), ("#CB39152D", 1.0)] {
            brush
                .gradient_stops()
                .add(GradientStop::with_color_and_offset(color(ink), offset));
        }
        tint.set_background(Some(brush.into()));
        tint.set_is_hit_test_visible(false);
        put(&stage, 0, 0, tint.upcast());
    }
    let identity = grid("*,Auto,Auto", "");
    let names = stack(3.0);
    names.children().add(
        strong(
            if m.current.title.is_empty() {
                "Nothing playing yet"
            } else {
                &m.current.title
            },
            if mobile { 20.0 } else { 32.0 },
            TEXT,
        )
        .upcast::<Control>(),
    );
    names
        .children()
        .add(label(&m.current.artist, 16.0, TEXT).upcast::<Control>());
    if !m.current.album.is_empty() {
        names
            .children()
            .add(label(&m.current.album, 12.0, MUTED).upcast::<Control>());
    }
    put(&identity, 0, 0, names.upcast());
    if !m.current.id.is_empty() {
        put(
            &identity,
            1,
            0,
            action(
                "Like current track",
                if m.current.liked { "heart-on" } else { "heart" },
                ACCENT,
                "like",
                &m.current.id,
            )
            .upcast(),
        );
    }
    if m.current.id.starts_with("spotify:") {
        put(
            &identity,
            2,
            0,
            action(
                "Open current track in Spotify",
                "external",
                MUTED,
                "spotify-link",
                &m.current.id,
            )
            .upcast(),
        );
    }
    let art_size = if mobile { 96.0 } else { 158.0 };
    let card = cover(&m.current.art, art_size, 0.0);
    card.set_width(art_size);
    card.set_border_thickness(Thickness::uniform(2.0));
    card.set_border_brush(Some(brush(TEXT)));
    card.set_effect(Some(Rc::new(ImmutableDropShadowEffect::new(
        0.0,
        5.0,
        16.0,
        color("#000000"),
        0.65,
    ))));
    let focus = grid("Auto,*", "");
    put(&focus, 0, 0, card.upcast());
    center(&identity);
    identity.set_margin(Thickness::new(13.0, 0.0, 0.0, 0.0));
    put(&focus, 1, 0, identity.upcast());
    let frame = surface("Transparent", 0.0, 0.0);
    frame.set_child(focus);
    frame.set_horizontal_alignment(H::Left);
    frame.set_vertical_alignment(V::Bottom);
    frame.set_max_width(if mobile {
        width - 28.0
    } else {
        (width * 0.61).min(790.0)
    });
    frame.set_margin(Thickness::new(
        if mobile { 14.0 } else { 55.0 },
        0.0,
        14.0,
        if mobile { 26.0 } else { 72.0 },
    ));
    if animate {
        effects::entrance(&frame, 32.0, 0);
    }
    put(&stage, 0, 0, frame.upcast());
    let p = stack(10.0);
    let q = grid("*,Auto", "");
    put(&q, 0, 0, strong("Up next", 15.0, TEXT).upcast());
    if !m.queue.is_empty() {
        put(
            &q,
            1,
            0,
            kicker(&format!("{:02} TRACKS", m.queue.len()), DIM).upcast(),
        );
    }
    p.children().add(q.upcast::<Control>());
    if m.queue.is_empty() {
        p.children()
            .add(label("Your queue is empty.", 12.0, DIM).upcast::<Control>());
    }
    p.children().add(track_list(&m.queue, true));
    let page = stack(0.0);
    page.children().add(stage.clone().upcast::<Control>());
    let queue = surface(if mobile { BG } else { "#D51B121D" }, 13.0, 0.0);
    if mobile {
        queue.set_child(p);
        page.children().add(queue.upcast::<Control>());
    } else {
        let list = scroll();
        list.set_content(Some(Rc::new(p.upcast::<Control>())));
        queue.set_child(list);
        queue.set_width((width * 0.27).clamp(255.0, 356.0));
        queue.set_max_height(316.0);
        queue.set_horizontal_alignment(H::Right);
        queue.set_vertical_alignment(V::Center);
        queue.set_margin(Thickness::new(0.0, 12.0, 26.0, 12.0));
        put(&stage, 0, 0, queue.upcast());
    }
    page.upcast()
}
fn time(s: f64) -> String {
    let s = s.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}
fn slider(name: &str, max: f64, value: f64, code: &'static str) -> Ref<Slider> {
    let s = Slider::new();
    s.set_maximum(max.max(1.0));
    s.set_range_value(value);
    s.set_min_width(0.0);
    s.set_height(25.0);
    AutomationProperties::set_name(&s, Some(name));
    let w = s.downgrade();
    s.value_changed(move |_, _| {
        if !TICKING.get()
            && let Some(s) = w.upgrade()
        {
            send(code, &s.value().to_string());
        }
    });
    s
}
fn player(m: &Model, width: f64) -> Ref<Control> {
    let compact = width < 920.0;
    let g = grid(
        if compact { "*,Auto" } else { "*,*,*,Auto" },
        if compact { "42,25" } else { "" },
    );
    g.set_margin(Thickness::new(
        if compact { 10.0 } else { 24.0 },
        5.0,
        if compact { 10.0 } else { 24.0 },
        2.0,
    ));
    center(&g);

    let duration = label(&time(m.current.duration), 10.0, DIM);
    duration.set_horizontal_alignment(H::Right);
    center(&duration);
    let elapsed = label("0:00", 10.0, DIM);
    center(&elapsed);
    let bar = slider("Seek", m.current.duration, 0.0, "seek");
    let seek = grid("34,*,34", "");
    put(&seek, 0, 0, elapsed.clone().upcast());
    put(&seek, 1, 0, bar.clone().upcast());
    put(&seek, 2, 0, duration.upcast());
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        ui.seek = Some(bar);
        ui.elapsed = Some(elapsed);
    });

    let title = if m.current.title.is_empty() {
        "Choose something to play"
    } else {
        &m.current.title
    };
    let name = stack(1.0);
    name.children()
        .add(strong(title, 12.0, TEXT).upcast::<Control>());
    name.children()
        .add(label(&m.current.artist, 10.5, MUTED).upcast::<Control>());
    let playing = button("Open Now Playing", name.upcast(), false, || {
        send("navigate", "queue")
    });
    playing.set_horizontal_content_alignment(H::Left);
    playing.set_padding(Thickness::uniform(0.0));
    center(&playing);

    let size = if compact { 33.0 } else { 42.0 };
    let art = cover(&m.current.art, size, 0.0);
    art.set_width(size);
    center(&art);
    let identity = grid(if compact { "38,*" } else { "49,*" }, "");
    put(&identity, 0, 0, art.upcast());
    if compact {
        put(&identity, 1, 0, playing.upcast());
        put(&g, 0, 0, identity.upcast());
        Grid::set_column_span(&seek, 2);
        put(&g, 0, 1, seek.upcast());
    } else {
        let controls = grid("*,Auto", "Auto,Auto");
        put(&controls, 0, 0, playing.upcast());
        if !m.current.id.is_empty() {
            put(
                &controls,
                1,
                0,
                action(
                    "Like current track",
                    if m.current.liked { "heart-on" } else { "heart" },
                    if m.current.liked { ACCENT } else { MUTED },
                    "like",
                    &m.current.id,
                )
                .upcast(),
            );
        }
        Grid::set_column_span(&seek, 2);
        put(&controls, 0, 1, seek.upcast());
        put(&identity, 1, 0, controls.upcast());
        identity.set_width(315.0);
        identity.set_horizontal_alignment(H::Center);
        Grid::set_column_span(&identity, 4);
        put(&g, 0, 0, identity.upcast());

        let left = row(3.0);
        center(&left);
        left.children().add(
            action(
                "Select playback device",
                "devices",
                MUTED,
                "navigate",
                "devices",
            )
            .upcast::<Control>(),
        );
        left.children()
            .add(action("Queue", "queue", MUTED, "navigate", "queue").upcast::<Control>());
        Grid::set_column_span(&left, 2);
        put(&g, 0, 0, left.upcast());
    }

    let buttons = row(2.0);
    center(&buttons);
    if !compact {
        buttons.children().add(
            action(
                "Repeat",
                "repeat",
                if m.repeat { ACCENT } else { MUTED },
                "repeat",
                "",
            )
            .upcast::<Control>(),
        );
        buttons.children().add(
            action(
                "Shuffle",
                "shuffle",
                if m.shuffle { ACCENT } else { MUTED },
                "shuffle",
                "",
            )
            .upcast::<Control>(),
        );
    }
    buttons
        .children()
        .add(action("Previous track", "prev", TEXT, "previous", "").upcast::<Control>());
    let toggle = icon_button(
        if m.playing { "Pause" } else { "Play" },
        if m.playing { "pause" } else { "play" },
        TEXT,
        14.0,
        || send("toggle", ""),
    );
    toggle.set_width(36.0);
    toggle.set_height(36.0);
    toggle.set_corner_radius(CornerRadius::uniform(18.0));
    toggle.set_border_brush(Some(brush(MUTED)));
    toggle.set_border_thickness(Thickness::uniform(1.5));
    buttons.children().add(toggle.upcast::<Control>());
    buttons
        .children()
        .add(action("Next track", "next", TEXT, "next", "").upcast::<Control>());
    if compact {
        buttons
            .children()
            .add(action("Queue", "queue", MUTED, "navigate", "queue").upcast::<Control>());
    } else {
        buttons
            .children()
            .add(icon("volume", ACCENT, 14.0).upcast::<Control>());
        let volume = slider("Volume", 100.0, m.volume, "volume");
        volume.set_width(62.0);
        center(&volume);
        buttons.children().add(volume.upcast::<Control>());
    }
    put(&g, if compact { 1 } else { 3 }, 0, buttons.upcast());
    g.upcast()
}
fn refresh(body: bool, playback: bool, animate: bool) {
    if !body && !playback {
        return;
    }
    let (m, width, host, main, footer, nav, reduced, chrome, transport, fire, search, account) = UI
        .with(|ui| {
            let ui = ui.borrow();
            (
                ui.model.clone(),
                ui.width,
                ui.body.clone(),
                ui.main.clone(),
                ui.footer.clone(),
                ui.nav.clone(),
                ui.reduced,
                ui.chrome.clone(),
                ui.transport.clone(),
                ui.fire.clone(),
                ui.search.clone(),
                ui.account_label.clone(),
            )
        });
    let collection = is_collection(&m.page);
    if body {
        LIGHT_PAGE.set(collection);
        for shell in [chrome, transport].into_iter().flatten() {
            shell.set_background(Some(brush(BG)));
        }
        if let Some(search) = search {
            search.set_foreground(Some(brush(TEXT)));
            search.set_placeholder_foreground(Some(brush(MUTED)));
            search.set_background(Some(brush(if collection { "#F4EFF3" } else { "#211923" })));
        }
        if let Some(account) = account {
            account.set_foreground(Some(brush(TEXT)));
        }
        for (route, text) in nav {
            let selected = m.page == route || (route == "library" && collection);
            text.set_foreground(Some(brush(if selected { TEXT } else { MUTED })));
        }
        if let Some(host) = host {
            UI.with(|ui| ui.borrow_mut().now_stage = None);
            host.set_background(Some(brush(BG)));
            if m.page == "library" {
                let existing = UI.with(|ui| ui.borrow().collection.clone());
                let view = existing.unwrap_or_else(|| {
                    let view = Rc::new(CollectionView::new(&m, width, animate && !reduced));
                    UI.with(|ui| ui.borrow_mut().collection = Some(view.clone()));
                    if let Some(main) = &main {
                        main.set_content(None);
                    }
                    host.set_child(view.root.clone());
                    view
                });
                view.update(&m);
            } else if let Some(main) = main {
                UI.with(|ui| ui.borrow_mut().collection = None);
                let child = main.clone().upcast::<Control>();
                if host.child().as_ref() != Some(&child) {
                    host.set_child(child);
                }
                let view = main_page(&m, width, animate && !reduced);
                main.set_content(Some(Rc::new(view)));
            }
        }
        LIGHT_PAGE.set(false);
    }
    if playback {
        if let Some(footer) = footer {
            LIGHT_PAGE.set(collection);
            footer.set_child(player(&m, width));
            LIGHT_PAGE.set(false);
        }
        if let Some(fire) = fire {
            fire.set_playing(m.playing);
        }
    }
}

fn is_collection(page: &str) -> bool {
    matches!(page, "library" | "liked" | "detail")
}

#[cfg(target_os = "emscripten")]
#[wasm_bindgen(js_name = clearReveal)]
pub fn clear_reveal() {
    if let Some(glow) = UI.with(|ui| ui.borrow().reveal.as_ref().and_then(WeakRef::upgrade)) {
        glow.set_opacity(0.0);
    }
}

#[cfg(target_os = "emscripten")]
#[wasm_bindgen(js_name = runMain)]
pub fn run_main(json: &str, width: f64, reduced: bool) {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        ui.model = serde_json::from_str(json).unwrap_or_default();
        ui.width = width;
        ui.reduced = reduced;
    });
    AppBuilder::configure::<App>()
        .with_inter_font()
        .with(Rc::new(FontManagerOptions {
            default_family_name: Some("fonts:Inter#Inter".into()),
            ..Default::default()
        }))
        .start_browser_app(
            "out",
            Some(BrowserPlatformOptions {
                accessibility: BrowserAccessibilityMode::Enabled,
                ..Default::default()
            }),
        );
}
#[cfg(target_os = "emscripten")]
#[wasm_bindgen(js_name = updateModel)]
pub fn update_model(json: &str, body: bool, playback: bool, reset: bool) {
    let Ok(model) = serde_json::from_str::<Model>(json) else {
        return;
    };
    apply_model(model, body, playback, reset);
}
pub(crate) fn apply_model(model: Model, body: bool, playback: bool, reset: bool) {
    let (connected_changed, body, playback) = UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let previous = &ui.model;
        let connected_changed = previous.connected != model.connected;
        let body = body
            && (reset
                || match model.page.as_str() {
                    "library" => {
                        previous.page != model.page
                            || previous.items != model.items
                            || previous.collections != model.collections
                    }
                    "home" => {
                        previous.page != model.page
                            || previous.items != model.items
                            || previous.collections != model.collections
                            || previous.pins != model.pins
                            || previous.recent != model.recent
                            || previous.status != model.status
                    }
                    "queue" => {
                        previous.page != model.page
                            || previous.queue != model.queue
                            || previous.current != model.current
                            || previous.playing != model.playing
                            || previous.status != model.status
                    }
                    _ => true,
                });
        let playback = is_collection(&previous.page) != is_collection(&model.page)
            || (playback
                && (previous.current != model.current
                    || previous.playing != model.playing
                    || previous.shuffle != model.shuffle
                    || previous.repeat != model.repeat
                    || previous.volume != model.volume));
        ui.model = model;
        (connected_changed, body, playback)
    });
    if connected_changed {
        build();
    } else {
        refresh(body, playback, reset);
    }
    if reset {
        let main = UI.with(|s| s.borrow().main.clone());
        if let Some(main) = main {
            main.set_offset(ferroui_base::Vector::default());
        }
    }
}
#[cfg_attr(target_os = "emscripten", wasm_bindgen)]
pub fn resize(width: f64) {
    let old = UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let old = ui.width;
        ui.width = width;
        old
    });
    if [600.0, 790.0, 920.0, 980.0, 1050.0, 1360.0]
        .iter()
        .any(|b| (old < *b) != (width < *b))
    {
        build();
    }
}
#[cfg_attr(target_os = "emscripten", wasm_bindgen)]
pub fn tick(position: f64) {
    let (bar, t) = UI.with(|ui| {
        let ui = ui.borrow();
        (ui.seek.clone(), ui.elapsed.clone())
    });
    TICKING.set(true);
    if let Some(bar) = bar
        && !bar.is_dragging()
    {
        bar.set_range_value(position);
    }
    if let Some(t) = t {
        let value = time(position);
        if t.text().as_deref() != Some(value.as_str()) {
            t.set_text(Some(&value));
        }
    }
    TICKING.set(false);
}
/// Receives the latest window of mono samples from the page's audio graph. An empty window means
/// that nothing is audible. `listening` is false when the page has no way to hear the music.
#[cfg(target_os = "emscripten")]
#[wasm_bindgen]
pub fn spectrum(samples: &[f32], rate: f64, listening: bool) {
    crate::spectrum::push(samples, rate as u32);
    crate::spectrum::set_listening(listening);
}
#[cfg_attr(target_os = "emscripten", wasm_bindgen(js_name = setImage))]
pub fn set_image(url: &str, bytes: &[u8]) {
    if let Ok(bitmap) = Bitmap::from_stream(&mut Cursor::new(bytes)) {
        let bitmap = Rc::new(bitmap);
        let waiting = UI.with(|ui| {
            let mut ui = ui.borrow_mut();
            if !ui.art.contains_key(url) {
                if ui.art.len() >= 96
                    && let Some(oldest) = ui.art_order.pop_front()
                {
                    ui.art.remove(&oldest);
                }
                ui.art_order.push_back(url.to_owned());
            }
            ui.art.insert(url.into(), bitmap.clone());
            ui.waiting.remove(url).unwrap_or_default()
        });
        for w in waiting {
            if let Some(i) = w.upgrade() {
                i.set_source(Some(bitmap.clone()));
            }
        }
    } else {
        UI.with(|ui| ui.borrow_mut().waiting.remove(url));
    }
}

#[cfg_attr(target_os = "emscripten", wasm_bindgen(js_name = focusSearch))]
pub fn focus_search() {
    let search = UI.with(|ui| ui.borrow().search.clone());
    if let Some(search) = search {
        search.focus();
    }
}

#[cfg(not(target_os = "emscripten"))]
pub fn run_desktop() -> std::process::ExitCode {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        ui.model = crate::desktop::initial_model();
        ui.width = 1350.0;
    });
    let args: Vec<_> = std::env::args().skip(1).collect();
    let status = AppBuilder::configure::<App>()
        .use_platform_detect()
        .with_inter_font()
        .with(Rc::new(FontManagerOptions {
            default_family_name: Some("fonts:Inter#Inter".into()),
            ..Default::default()
        }))
        .start_with_classic_desktop_lifetime(&args);
    std::process::ExitCode::from(status as u8)
}
