use ferroui_base::rendering::composition::ICompositionObjectAnimations;

use ferroui_base::{
    CornerRadius, FerroObjectImpl, Ref, RelativePoint, RelativeUnit, Thickness, WeakRef,
    ferro_class, ferro_impl_classes,
    input::InputElement,
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
    ColumnDefinitions, Control, Grid, Image, NewApplication, RowDefinitions, ScrollViewer, Slider,
    StackPanel, TextBlock, TextBox,
    automation::AutomationProperties,
    hosting::ElementCompositionPreview,
    media::{
        ApplicationTheme, RevealBackgroundBrush, RevealBorderBrush, RevealBrush, RevealBrushState,
    },
    primitives::ScrollBarVisibility,
    shapes::Path,
};
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
    time::Duration,
};
#[cfg(target_os = "emscripten")]
use wasm_bindgen::prelude::*;

const BG: &str = "#0D1013";
const PANEL: &str = "#14191E";
const EDGE: &str = "#242C33";
const TEXT: &str = "#F1F5F5";
const MUTED: &str = "#A2AEB6";
const DIM: &str = "#83919B";
const ACCENT: &str = "#75F0B0";

#[derive(Clone, Default, Deserialize)]
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
    main: Option<Ref<ScrollViewer>>,
    side: Option<Ref<Border>>,
    right: Option<Ref<Border>>,
    footer: Option<Ref<Border>>,
    search: Option<Ref<TextBox>>,
    seek: Option<Ref<Slider>>,
    elapsed: Option<Ref<TextBlock>>,
    art: HashMap<String, Rc<Bitmap>>,
    waiting: HashMap<String, Vec<WeakRef<Image>>>,
}
thread_local! {
    static UI: RefCell<Ui> = RefCell::default();
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
        this.set_name(Some("SpotiFe".into()));
        this.set_requested_theme_variant(Some(ThemeVariant::dark()));
        this.styles().add(WinUITheme::new().as_style());
        for (key, ink) in [
            ("SliderTrackValueFill", ACCENT),
            ("SliderTrackValueFillPointerOver", "#9EF6C9"),
            ("SliderTrackValueFillPressed", "#4DCE92"),
            ("SliderThumbBackground", ACCENT),
            ("SliderThumbBackgroundPointerOver", "#9EF6C9"),
            ("SliderThumbBackgroundPressed", "#4DCE92"),
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
            window.set_title(Some("SpotiFe".into()));
            window.set_width(1350.0);
            window.set_height(900.0);
            window.set_min_width(380.0);
            window.set_min_height(450.0);
            window.set_content(Some(Control::boxed(shell)));
            crate::desktop::window_ready(&window);
            desktop.set_main_window(Some(window));
        }
        Self::parent_on_framework_initialization_completed(this);
    }
}
fn create_shell() -> Ref<Border> {
    let host = surface(BG, 0.0, 0.0);
    UI.with(|ui| ui.borrow_mut().shell = Some(host.clone()));
    build();
    host
}

fn color(s: &str) -> Color {
    Color::parse(s).unwrap()
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
fn stroke(b: &Border, ink: &str) {
    b.set_border_brush(Some(brush(ink)));
    b.set_border_thickness(Thickness::uniform(1.0));
}
fn gradient(b: &Border, stops: &[(&str, f64)]) {
    let g = LinearGradientBrush::new();
    g.set_end_point(RelativePoint::new(1.0, 0.0, RelativeUnit::Relative));
    for (c, n) in stops {
        g.gradient_stops()
            .add(GradientStop::with_color_and_offset(color(c), *n));
    }
    b.set_background(Some(g.into()));
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
fn icon(name: &str, ink: &str, size: f64) -> Ref<Path> {
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
    let p = Path::new();
    p.set_data(Geometry::parse(path).ok());
    p.set_width(size);
    p.set_height(size);
    p.set_stretch(Stretch::Uniform);
    p.set_vertical_alignment(V::Center);
    if fill {
        p.set_fill(Some(brush(ink)));
    } else {
        p.set_stroke(Some(brush(ink)));
        p.set_stroke_thickness(1.65);
    }
    p
}
fn button(name: &str, content: Ref<Control>, primary: bool, f: impl Fn() + 'static) -> Ref<Button> {
    let b = Button::new();
    b.set_content(Some(Control::boxed(content)));
    b.set_padding(Thickness::new(10.0, 6.0, 10.0, 6.0));
    b.set_corner_radius(CornerRadius::uniform(8.0));
    b.set_background(Some(brush(if primary { ACCENT } else { "Transparent" })));
    if primary {
        b.resources().set(
            "ButtonBackgroundPointerOver",
            Some(Rc::new(brush("#A2F8CA"))),
        );
        b.resources()
            .set("ButtonBackgroundPressed", Some(Rc::new(brush("#54CF94"))));
    }
    b.set_border_thickness(Thickness::uniform(0.0));
    b.set_horizontal_content_alignment(H::Center);
    AutomationProperties::set_name(&b, Some(name));
    b.click(move |_, _| f());
    b
}
fn action(name: &str, glyph: &str, ink: &str, code: &'static str, arg: &str) -> Ref<Button> {
    let arg = arg.to_owned();
    let b = button(name, icon(glyph, ink, 16.0).upcast(), false, move || {
        send(code, &arg)
    });
    b.set_width(35.0);
    b.set_height(34.0);
    b.set_padding(Thickness::uniform(0.0));
    b
}
fn pill(name: &str, glyph: &str, primary: bool, code: &'static str, arg: &str) -> Ref<Button> {
    let ink = if primary { "#10241A" } else { TEXT };
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
        b.set_background(Some(brush("#252E34")));
    }
    b
}
fn reveal(b: &Ref<Border>) {
    let fill = RevealBackgroundBrush::new();
    fill.set_color(color("#0875F0B0"));
    fill.set_fallback_color(color("Transparent"));
    fill.set_target_theme(ApplicationTheme::Dark);
    let edge = RevealBorderBrush::new();
    edge.set_color(color("#2075F0B0"));
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
fn fade(b: &Ref<Border>) {
    if UI.with(|s| s.borrow().reduced) {
        return;
    }
    let weak = b.downgrade();
    let _ = b
        .attached_to_visual_tree(move |_| {
            let weak = weak.clone();
            let _ = ferroui_base::threading::Dispatcher::ui_thread()
                .invoke_async_local_with_priority(
                    move || {
                        let Some(b) = weak.upgrade() else { return };
                        let Some(v) = ElementCompositionPreview::get_element_visual(&b) else {
                            return;
                        };
                        let a = v.compositor().create_scalar_key_frame_animation();
                        a.insert_key_frame(0.0, 0.25);
                        a.insert_key_frame(1.0, 1.0);
                        a.set_duration(Duration::from_millis(240));
                        v.start_animation("Opacity", &*a);
                    },
                    ferroui_base::threading::DispatcherPriority::BACKGROUND,
                );
        })
        .detach();
}
fn cover(url: &str, height: f64, radius: f64) -> Ref<Border> {
    let external = url.starts_with("https://");
    let b = surface("#21342E", 0.0, if external { 0.0 } else { radius });
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
            ui.waiting
                .entry(url.to_owned())
                .or_default()
                .push(image.downgrade());
            true
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
    s
}
fn build() {
    let (width, host, model) = UI.with(|s| {
        let s = s.borrow();
        (s.width, s.shell.clone().unwrap(), s.model.clone())
    });
    let mobile = width < 790.0;
    let right_visible = width >= 1120.0;
    let root = grid("", if mobile { "56,*,77" } else { "65,*,92" });
    root.set_clip_to_bounds(true);
    let body = grid(
        if right_visible {
            "204,*,278"
        } else if mobile {
            "*"
        } else {
            "204,*"
        },
        "",
    );
    body.set_margin(if mobile {
        Thickness::new(8.0, 0.0, 8.0, 6.0)
    } else {
        Thickness::new(12.0, 0.0, 12.0, 8.0)
    });
    let side = surface(PANEL, 0.0, 12.0);
    if !mobile {
        stroke(&side, EDGE);
        put(&body, 0, 0, side.clone().upcast());
    }
    let main = scroll();
    let main_frame = surface("#101519", 0.0, 12.0);
    stroke(&main_frame, EDGE);
    main_frame.set_clip_to_bounds(true);
    if !mobile {
        main_frame.set_margin(Thickness::new(8.0, 0.0, 0.0, 0.0));
    }
    main_frame.set_child(main.clone());
    put(&body, if mobile { 0 } else { 1 }, 0, main_frame.upcast());
    let right = surface(PANEL, 0.0, 12.0);
    if right_visible {
        stroke(&right, EDGE);
        right.set_margin(Thickness::new(8.0, 0.0, 0.0, 0.0));
        put(&body, 2, 0, right.clone().upcast());
    }
    let footer = surface("#11161A", 0.0, 0.0);
    let top = top_bar(mobile, &model);
    put(&root, 0, 0, top);
    put(&root, 0, 1, body.upcast());
    put(&root, 0, 2, footer.clone().upcast());
    UI.with(|s| {
        let mut s = s.borrow_mut();
        s.main = Some(main);
        s.side = Some(side);
        s.right = Some(right);
        s.footer = Some(footer);
    });
    refresh(true, true);
    host.set_child(root);
}
fn top_bar(mobile: bool, model: &Model) -> Ref<Control> {
    let g = grid(if mobile { "Auto,*,Auto" } else { "204,*,Auto" }, "");
    g.set_margin(Thickness::new(
        if mobile { 10.0 } else { 18.0 },
        0.0,
        if mobile { 10.0 } else { 18.0 },
        0.0,
    ));
    let brand = row(9.0);
    let mark = surface(ACCENT, 7.0, 9.0);
    mark.set_child(icon("logo", "#11261C", 17.0));
    brand.children().add(mark.upcast::<Control>());
    if !mobile {
        let t = strong("SpotiFe", 20.0, TEXT);
        center(&t);
        brand.children().add(t.upcast::<Control>());
    }
    let b = button("Home", brand.upcast(), false, || send("navigate", "home"));
    center(&b);
    b.set_horizontal_alignment(H::Left);
    b.set_padding(Thickness::uniform(0.0));
    put(&g, 0, 0, b.upcast());
    let search = TextBox::new();
    search.set_min_width(0.0);
    search.set_max_width(if mobile { 340.0 } else { 420.0 });
    search.set_margin(Thickness::new(9.0, 0.0, 10.0, 0.0));
    search.set_height(36.0);
    search.set_placeholder_text(Some("Search songs, artists, albums…"));
    search.set_text(Some(&model.query));
    search.set_background(Some(brush("#1B2228")));
    search.set_corner_radius(CornerRadius::uniform(9.0));
    search.set_inner_left_content(Some(Control::boxed(icon("search", MUTED, 15.0))));
    center(&search);
    AutomationProperties::set_name(&search, Some("Search songs, artists, albums"));
    let w = search.downgrade();
    search.text_changed(move |_, _| {
        if let Some(t) = w.upgrade() {
            send("query", &t.text().unwrap_or_default());
        }
    });
    put(&g, 1, 0, search.clone().upcast());
    UI.with(|ui| ui.borrow_mut().search = Some(search));
    let tools = row(if mobile { 0.0 } else { 5.0 });
    center(&tools);
    if mobile {
        tools.children().add(
            action("Your library", "library", MUTED, "navigate", "library").upcast::<Control>(),
        );
    }
    if !mobile {
        tools
            .children()
            .add(pill("Devices", "devices", false, "navigate", "devices").upcast::<Control>());
    }
    tools.children().add(if mobile {
        action("Spotify settings", "devices", ACCENT, "settings", "").upcast::<Control>()
    } else {
        pill(
            if model.connected {
                "Account"
            } else {
                "Connect Spotify"
            },
            "logo",
            true,
            "settings",
            "",
        )
        .upcast::<Control>()
    });
    put(&g, 2, 0, tools.upcast());
    g.upcast()
}
fn sidebar(m: &Model) -> Ref<Control> {
    let p = stack(4.0);
    p.set_margin(Thickness::new(12.0, 16.0, 12.0, 12.0));
    for (title, glyph, page) in [
        ("Home", "home", "home"),
        ("Browse", "search", "search"),
        ("Your library", "library", "library"),
    ] {
        let b = pill(title, glyph, false, "navigate", page);
        b.set_horizontal_alignment(H::Stretch);
        b.set_horizontal_content_alignment(H::Left);
        b.set_background(Some(brush(if m.page == page {
            "#233A31"
        } else {
            "Transparent"
        })));
        p.children().add(b.upcast::<Control>());
    }
    let l = kicker("YOUR COLLECTION", DIM);
    l.set_margin(Thickness::new(10.0, 19.0, 0.0, 5.0));
    p.children().add(l.upcast::<Control>());
    p.children()
        .add(side_item("Liked songs", "", "liked", "navigate").upcast::<Control>());
    for item in m.collections.iter().take(8) {
        p.children()
            .add(side_item(&item.title, &item.art, &item.id, "open").upcast::<Control>());
    }
    let s = scroll();
    s.set_content(Some(Rc::new(p.upcast::<Control>())));
    s.upcast()
}
fn side_item(title: &str, art: &str, id: &str, code: &'static str) -> Ref<Button> {
    let g = grid("31,*", "");
    let img = cover(art, 28.0, 5.0);
    img.set_width(28.0);
    put(&g, 0, 0, img.upcast());
    let t = strong(title, 11.0, MUTED);
    center(&t);
    t.set_margin(Thickness::new(7.0, 0.0, 0.0, 0.0));
    put(&g, 1, 0, t.upcast());
    let id = id.to_owned();
    let b = button(title, g.upcast(), false, move || send(code, &id));
    b.set_padding(Thickness::new(6.0, 4.0, 6.0, 4.0));
    b.set_horizontal_content_alignment(H::Stretch);
    b.set_horizontal_alignment(H::Stretch);
    b
}
fn header(title: &str, detail: &str) -> Ref<Control> {
    let g = grid("*,Auto", "");
    put(&g, 0, 0, strong(title, 23.0, TEXT).upcast());
    let t = kicker(detail, DIM);
    center(&t);
    put(&g, 1, 0, t.upcast());
    g.upcast()
}
fn hero(m: &Model, narrow: bool) -> Ref<Control> {
    let featured = m.collections.first().cloned().unwrap_or_default();
    let h = if narrow { 218.0 } else { 242.0 };
    let b = surface("#153A30", 0.0, 13.0);
    stroke(&b, "#35594D");
    b.set_height(h);
    b.set_clip_to_bounds(true);
    let g = grid("", "");
    if !m.connected {
        let art = cover("art/hero.jpg", h, 12.0);
        art.set_is_hit_test_visible(false);
        put(&g, 0, 0, art.upcast());
    }
    let wash = surface("Transparent", 0.0, 12.0);
    gradient(
        &wash,
        &[
            ("#F70D211B", 0.0),
            ("#E6153028", 0.37),
            ("#571A392F", 0.75),
            ("#131A392F", 1.0),
        ],
    );
    wash.set_is_hit_test_visible(false);
    put(&g, 0, 0, wash.upcast());
    let p = stack(7.0);
    p.set_vertical_alignment(V::Center);
    p.set_margin(Thickness::new(
        if narrow { 18.0 } else { 29.0 },
        0.0,
        12.0,
        0.0,
    ));
    p.children().add(
        kicker(
            if m.connected {
                "YOUR SPOTIFY LIBRARY"
            } else {
                "DEMO MIX"
            },
            ACCENT,
        )
        .upcast::<Control>(),
    );
    let headline = strong(
        if m.connected {
            "Your music"
        } else {
            "After Hours"
        },
        if narrow { 38.0 } else { 56.0 },
        TEXT,
    );
    headline.set_letter_spacing(-1.5);
    p.children().add(headline.upcast::<Control>());
    let sub = label(
        if m.connected {
            "Pick up where you left off."
        } else {
            "Warm synths, slow pulses, and room to breathe."
        },
        13.0,
        "#C7DED6",
    );
    sub.set_text_wrapping(TextWrapping::Wrap);
    p.children().add(sub.upcast::<Control>());
    let r = row(7.0);
    r.set_margin(Thickness::new(0.0, 11.0, 0.0, 0.0));
    if !featured.id.is_empty() {
        let play = pill("Play mix", "play", true, "play", &featured.id);
        play.set_effect(Some(Rc::new(ImmutableDropShadowEffect::new(
            0.0,
            3.0,
            16.0,
            color(ACCENT),
            0.27,
        ))));
        r.children().add(play.upcast::<Control>());
        r.children()
            .add(pill("Explore", "arrow", false, "open", &featured.id).upcast::<Control>());
    } else {
        r.children()
            .add(pill("Find music", "search", true, "navigate", "search").upcast::<Control>());
    }
    p.children().add(r.upcast::<Control>());
    put(&g, 0, 0, p.upcast());
    b.set_child(g);
    fade(&b);
    b.upcast()
}
fn collection_cards(items: &[Item], cols: usize) -> Ref<Control> {
    let rows = stack(8.0);
    for chunk in items.chunks(cols) {
        let g = grid(&vec!["*"; cols].join(","), "");
        for (idx, item) in chunk.iter().enumerate() {
            let card = surface(PANEL, 0.0, 10.0);
            card.set_margin(Thickness::new(
                if idx == 0 { 0.0 } else { 4.0 },
                0.0,
                if idx + 1 == cols { 0.0 } else { 4.0 },
                0.0,
            ));
            let light = surface("Transparent", 0.0, 10.0);
            reveal(&light);
            let p = stack(5.0);
            let image = cover(&item.art, if cols > 2 { 108.0 } else { 95.0 }, 6.0);
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
        rows.children().add(g.upcast::<Control>());
    }
    rows.upcast()
}
fn track(item: &Item, index: usize, compact: bool) -> Ref<Control> {
    let light = surface("Transparent", 0.0, 7.0);
    reveal(&light);
    let g = grid(
        if compact {
            "*,35"
        } else if item.id.starts_with("spotify:") {
            "*,32,32,32,42"
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
fn main_page(m: &Model, width: f64) -> Ref<Control> {
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
            let content = strong("Sign in with Spotify", 12.0, "#11261C");
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
    let cols = if width >= 980.0 && (width < 1120.0 || width > 1270.0) {
        4
    } else if width > 600.0 {
        3
    } else {
        2
    };
    if m.page == "home" {
        page.children().add(hero(m, mobile));
        if !m.collections.is_empty() {
            page.children().add(header(
                if m.connected {
                    "Your playlists"
                } else {
                    "Made for the moment"
                },
                if m.connected { "" } else { "CURATED MIXES" },
            ));
            page.children().add(collection_cards(
                &m.collections[..m.collections.len().min(cols)],
                cols,
            ));
        }
        page.children().add(header(
            if m.connected {
                "Liked songs"
            } else {
                "On rotation"
            },
            "TRACKS",
        ));
    } else if m.page == "queue" {
        return now_playing(m, mobile);
    } else {
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
            if has_tracks || m.detail.starts_with("spotify:") {
                page.children().add(actions.upcast::<Control>());
            }
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
        .filter(|i| !i.kind.is_empty() && i.kind != "track")
        .cloned()
        .collect::<Vec<_>>();
    if !cards.is_empty() {
        page.children().add(collection_cards(&cards, cols));
    }
    let list = stack(2.0);
    for (idx, item) in m
        .items
        .iter()
        .filter(|i| i.kind.is_empty() || i.kind == "track")
        .enumerate()
    {
        list.children().add(track(item, idx, false));
    }
    page.children().add(list.upcast::<Control>());
    page.upcast()
}
fn now_playing(m: &Model, mobile: bool) -> Ref<Control> {
    let p = stack(11.0);
    p.set_margin(Thickness::uniform(14.0));
    let heading = grid("*,Auto", "");
    put(&heading, 0, 0, strong("Now playing", 15.0, TEXT).upcast());
    let live = kicker(if m.playing { "PLAYING" } else { "READY" }, ACCENT);
    center(&live);
    put(&heading, 1, 0, live.upcast());
    p.children().add(heading.upcast::<Control>());
    let a = cover(&m.current.art, if mobile { 230.0 } else { 218.0 }, 9.0);
    a.set_effect(Some(Rc::new(ImmutableDropShadowEffect::new(
        0.0,
        3.0,
        12.0,
        color("#000000"),
        0.6,
    ))));
    p.children().add(a.upcast::<Control>());
    let identity = grid("*,Auto,Auto", "");
    let names = stack(3.0);
    names.children().add(
        strong(
            if m.current.title.is_empty() {
                "Nothing playing yet"
            } else {
                &m.current.title
            },
            18.0,
            TEXT,
        )
        .upcast::<Control>(),
    );
    names
        .children()
        .add(label(&m.current.artist, 12.0, MUTED).upcast::<Control>());
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
    p.children().add(identity.upcast::<Control>());
    let line = surface(EDGE, 0.0, 0.0);
    line.set_height(1.0);
    line.set_margin(Thickness::new(0.0, 4.0, 0.0, 4.0));
    p.children().add(line.upcast::<Control>());
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
    for (idx, item) in m.queue.iter().enumerate() {
        p.children().add(track(item, idx, true));
    }
    p.upcast()
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
    let compact = width < 690.0;
    let g = grid(
        if compact { "*,Auto" } else { "24*,52*,24*" },
        if compact { "42,25" } else { "" },
    );
    g.set_margin(Thickness::new(
        if compact { 10.0 } else { 16.0 },
        5.0,
        if compact { 10.0 } else { 16.0 },
        2.0,
    ));
    if !compact {
        center(&g);
    }
    let id = grid(if compact { "38,*" } else { "53,*,36" }, "");
    center(&id);
    let a = cover(&m.current.art, if compact { 33.0 } else { 47.0 }, 6.0);
    a.set_width(if compact { 33.0 } else { 47.0 });
    put(&id, 0, 0, a.upcast());
    let n = stack(2.0);
    center(&n);
    n.children().add(
        strong(
            if m.current.title.is_empty() {
                "Choose something to play"
            } else {
                &m.current.title
            },
            12.0,
            TEXT,
        )
        .upcast::<Control>(),
    );
    n.children()
        .add(label(&m.current.artist, 10.5, MUTED).upcast::<Control>());
    put(&id, 1, 0, n.upcast());
    if !compact && !m.current.id.is_empty() {
        put(
            &id,
            2,
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
    put(&g, 0, 0, id.upcast());
    let p = stack(0.0);
    let buttons = row(2.0);
    buttons.set_horizontal_alignment(H::Center);
    if !compact {
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
    let b = button(
        if m.playing { "Pause" } else { "Play" },
        icon(if m.playing { "pause" } else { "play" }, "#10241A", 16.0).upcast(),
        true,
        || send("toggle", ""),
    );
    b.set_width(34.0);
    b.set_height(34.0);
    b.set_corner_radius(CornerRadius::uniform(17.0));
    b.set_padding(Thickness::uniform(0.0));
    buttons.children().add(b.upcast::<Control>());
    buttons
        .children()
        .add(action("Next track", "next", TEXT, "next", "").upcast::<Control>());
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
    }
    buttons
        .children()
        .add(action("Queue", "queue", MUTED, "navigate", "queue").upcast::<Control>());
    if compact {
        put(&g, 1, 0, buttons.upcast());
    } else {
        p.children().add(buttons.upcast::<Control>());
    }
    let seek = grid("36,*,36", "");
    let elapsed = label("0:00", 10.0, DIM);
    center(&elapsed);
    let duration = label(&time(m.current.duration), 10.0, DIM);
    center(&duration);
    duration.set_horizontal_alignment(H::Right);
    let bar = slider("Seek", m.current.duration, 0.0, "seek");
    put(&seek, 0, 0, elapsed.clone().upcast());
    put(&seek, 1, 0, bar.clone().upcast());
    put(&seek, 2, 0, duration.upcast());
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        ui.seek = Some(bar);
        ui.elapsed = Some(elapsed);
    });
    if compact {
        Grid::set_column_span(&seek, 2);
        put(&g, 0, 1, seek.upcast());
    } else {
        p.children().add(seek.upcast::<Control>());
        p.set_max_width(495.0);
        put(&g, 1, 0, p.upcast());
        let tools = row(5.0);
        center(&tools);
        tools.set_horizontal_alignment(H::Right);
        tools.children().add(
            action(
                "Select playback device",
                "devices",
                MUTED,
                "navigate",
                "devices",
            )
            .upcast::<Control>(),
        );
        tools
            .children()
            .add(icon("volume", MUTED, 16.0).upcast::<Control>());
        let v = slider("Volume", 100.0, m.volume, "volume");
        v.set_width(78.0);
        tools.children().add(v.upcast::<Control>());
        put(&g, 2, 0, tools.upcast());
    }
    g.upcast()
}
fn refresh(body: bool, playback: bool) {
    let (m, width, main, side, right, footer) = UI.with(|ui| {
        let ui = ui.borrow();
        (
            ui.model.clone(),
            ui.width,
            ui.main.clone(),
            ui.side.clone(),
            ui.right.clone(),
            ui.footer.clone(),
        )
    });
    if body {
        if let Some(main) = main {
            main.set_content(Some(Rc::new(main_page(&m, width))));
        }
        if width >= 790.0
            && let Some(side) = side
        {
            side.set_child(sidebar(&m));
        }
    }
    if playback {
        if width >= 1120.0
            && let Some(right) = right
        {
            let s = scroll();
            s.set_content(Some(Rc::new(now_playing(&m, false))));
            right.set_child(s);
        }
        if let Some(footer) = footer {
            footer.set_child(player(&m, width));
        }
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
    let connected_changed = UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let changed = ui.model.connected != model.connected;
        ui.model = model;
        changed
    });
    if connected_changed {
        build();
    } else {
        refresh(body, playback);
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
    if [600.0, 690.0, 790.0, 980.0, 1120.0, 1271.0]
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
    if let Some(bar) = bar {
        bar.set_range_value(position);
    }
    if let Some(t) = t {
        t.set_text(Some(&time(position)));
    }
    TICKING.set(false);
}
#[cfg_attr(target_os = "emscripten", wasm_bindgen(js_name = setImage))]
pub fn set_image(url: &str, bytes: &[u8]) {
    if let Ok(bitmap) = Bitmap::from_stream(&mut Cursor::new(bytes)) {
        let bitmap = Rc::new(bitmap);
        let waiting = UI.with(|ui| {
            let mut ui = ui.borrow_mut();
            if ui.art.len() >= 120 {
                ui.art.clear();
            }
            ui.art.insert(url.into(), bitmap.clone());
            ui.waiting.remove(url).unwrap_or_default()
        });
        for w in waiting {
            if let Some(i) = w.upgrade() {
                i.set_source(Some(bitmap.clone()));
            }
        }
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
