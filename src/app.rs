use crate::{effects, view::mount};

#[cfg(not(target_os = "emscripten"))]
use ferroui_base::input::WindowDecorationsElementRole;
use ferroui_base::{
    BoxedValue, CornerRadius, FerroObjectImpl, Ref, RelativePoint, RelativeUnit, Thickness,
    WeakRef,
    data::{BindingMode, ReflectionBinding, TemplateBinding},
    ferro_class, ferro_impl_classes,
    input::{InputElement, NavigationMethod, gesture_recognizers::ScrollGestureRecognizer},
    instantiate,
    layout::{HorizontalAlignment as H, Orientation, VerticalAlignment as V},
    media::{
        Color, FontManagerOptions, FontWeight, Geometry, GradientStop, IBrush, LinearGradientBrush, ScaleTransform,
        Stretch, TextTrimming, TextWrapping, effects::ImmutableDropShadowEffect, imaging::Bitmap,
        immutable::ImmutableSolidColorBrush,
    },
    styling::ThemeVariant,
};
#[cfg(target_os = "emscripten")]
use ferroui_browser::{BrowserAccessibilityMode, BrowserAppBuilder, BrowserPlatformOptions};
#[cfg(not(target_os = "emscripten"))]
use ferroui_controls::chrome::WindowDecorationProperties;
use ferroui_controls::{
    AppBuilder, Application, ApplicationImpl, ApplicationImplExt, Border, Button, ContentControl,
    Control, Grid, Image, ItemsSource, NewApplication, RepeatButton, ScrollViewer, Slider,
    StackPanel, TextBlock, TextBox, ToolTip,
    automation::AutomationProperties,
    media::{
        ApplicationTheme, RevealBackgroundBrush, RevealBorderBrush, RevealBrush, RevealBrushState,
    },
    presenters::ContentPresenter,
    primitives::{RangeBase, ScrollBar, ScrollBarVisibility, TemplatedControl, Track},
    templates::{
        FuncControlTemplate, FuncDataTemplate, FuncTemplateNameScopeExtensions, IControlTemplate,
    },
};
use ferroui_controls_repeater::{ItemsRepeater, StackLayout};
#[cfg(not(target_os = "emscripten"))]
use ferroui_desktop::AppBuilderDesktopExtensions as _;
use ferroui_fonts_inter::AppBuilderExtension;
use ferroui_themes_winui::WinUITheme;
use ferroui_view::prelude::{self as ui, *};
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
#[cfg(not(target_os = "emscripten"))]
const DESKTOP_HEADER_HEIGHT: f64 = 48.0;
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
    pub(crate) collection_art: Vec<String>,
    #[cfg(target_os = "emscripten")]
    pub(crate) folder_name: String,
    #[cfg(target_os = "emscripten")]
    pub(crate) folder_notice: String,
    #[cfg(target_os = "emscripten")]
    pub(crate) folder_can_rescan: bool,
    pub(crate) queue: Vec<Item>,
    pub(crate) liked_music: Vec<Item>,
    pub(crate) recent: Vec<Item>,
    pub(crate) current: Item,
    pub(crate) playing: bool,
    pub(crate) shuffle: bool,
    pub(crate) repeat: bool,
    pub(crate) volume: f64,
    pub(crate) can_go_back: bool,
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
    back: Option<(Ref<Border>, (bool, bool))>,
    footer: Option<Ref<Border>>,
    chrome: Option<Ref<Border>>,
    #[cfg(not(target_os = "emscripten"))]
    caption_area: Option<WeakRef<Border>>,
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

#[cfg(not(target_os = "emscripten"))]
pub(crate) fn pick_music_folder(done: impl FnOnce(Result<Option<String>, String>) + 'static) -> Result<(), String> {
    use ferroui_base::{platform::storage::{FolderPickerOpenOptions, IStorageItem}, threading::{Dispatcher, DispatcherPriority}};
    use ferroui_controls::TopLevel;

    let shell = UI.with(|ui| ui.borrow().shell.clone()).map(|shell| shell.upcast::<ferroui_base::Visual>());
    let top = TopLevel::get_top_level(shell.as_deref()).ok_or("Could not open the folder picker.")?;
    let provider = top.storage_provider();
    let _ = Dispatcher::ui_thread().spawn_local_with_priority(async move {
        let result = provider.open_folder_picker_async(FolderPickerOpenOptions::new())
            .await.map_err(|_| "Could not open the folder picker.".to_owned())
            .and_then(|folders| folders.into_iter().next().map(|folder| {
                (folder.as_ref() as &dyn IStorageItem).try_get_local_path()
                    .ok_or_else(|| "Choose a folder on this computer.".into())
            }).transpose());
        done(result);
    }, DispatcherPriority::DEFAULT);
    Ok(())
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
        #[cfg(not(target_os = "emscripten"))]
        this.resources()
            .set("WindowCaptionBackground", Some(Rc::new(brush("#00000000"))));
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
            let shell = create_shell();
            shell.size_changed(|_, event| {
                let width = event.new_size().width;
                if event.width_changed() && width > 0.0 {
                    Dispatcher::ui_thread()
                        .post_local(move || resize(width), DispatcherPriority::NORMAL);
                }
            });
            let window = mount(
                ui::window()
                    .title(APP_NAME)
                    .width(1350.0)
                    .height(900.0)
                    .min_width(380.0)
                    .min_height(450.0),
            );
            install_desktop_title_bar(&window, shell);
            install_system_back(&window);
            crate::desktop::window_ready(&window);
            desktop.set_main_window(Some(window));
        }
        Self::parent_on_framework_initialization_completed(this);
    }
}

fn install_system_back(top: &ferroui_controls::TopLevel) {
    top.back_requested(|_, event| {
        if UI.with(|ui| ui.borrow().model.can_go_back) {
            event.set_handled(true);
            send("back", "");
        }
    });
}

#[cfg(target_os = "emscripten")]
#[wasm_bindgen(js_name = installBackNavigation)]
pub fn install_back_navigation() {
    let shell = UI.with(|ui| ui.borrow().shell.clone()).map(|shell| shell.upcast::<ferroui_base::Visual>());
    if let Some(top) = ferroui_controls::TopLevel::get_top_level(shell.as_deref()) {
        install_system_back(&top);
    }
}

#[cfg(not(target_os = "emscripten"))]
fn install_desktop_title_bar(window: &Ref<ferroui_controls::Window>, shell: Ref<Border>) {
    use ferroui_controls::{
        Window, WindowState,
        chrome::{TitleBarDecorations, WindowDrawnDecorations},
    };

    window.set_extend_client_area_title_bar_height_hint(DESKTOP_HEADER_HEIGHT);
    window.set_extend_client_area_to_decorations_hint(true);
    window.set_content(Some(Control::boxed(shell.clone())));

    let decorations = WindowDrawnDecorations::get_title_bar_decorations(window);
    let (window_ref, shell_ref) = (window.downgrade(), shell.downgrade());
    let update_chrome = move || {
        let (Some(window), Some(shell)) = (window_ref.upgrade(), shell_ref.upgrade()) else {
            return;
        };
        let unified = window.is_extended_into_window_decorations()
            && window.window_state() != WindowState::FullScreen;
        let margin = window.window_decoration_margin();
        if unified {
            shell.set_margin(Thickness::new(margin.left, 0.0, margin.right, margin.bottom));
            WindowDrawnDecorations::set_title_bar_decorations(
                &window,
                decorations & !TitleBarDecorations::TITLE,
            );
        } else {
            shell.set_margin(margin);
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

    // The toolkit-drawn caption buttons report their insets after layout.
    let window = window.downgrade();
    shell
        .layout_updated(move || {
            let Some(window) = window.upgrade() else { return };
            let area = UI.with(|ui| ui.borrow().caption_area.as_ref().and_then(WeakRef::upgrade));
            if let Some(area) = area {
                let insets = window.caption_button_insets();
                if area.padding() != insets {
                    area.set_padding(insets);
                }
                let role = if window.is_extended_into_window_decorations()
                    && window.window_state() != WindowState::FullScreen
                {
                    WindowDecorationsElementRole::TitleBar
                } else {
                    WindowDecorationsElementRole::None
                };
                if WindowDecorationProperties::get_element_role(&area) != role {
                    WindowDecorationProperties::set_element_role(&area, role);
                }
            }
        })
        .detach();
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
fn text_view(s: &str, size: f64, ink: &str) -> El<TextBlock> {
    ui::text_block()
        .text(s)
        .font_size(size)
        .foreground(brush(ink))
        .text_trimming(<dyn TextTrimming>::character_ellipsis())
}
fn label(s: &str, size: f64, ink: &str) -> Ref<TextBlock> {
    mount(text_view(s, size, ink))
}
fn metro_title(text: &str, size: f64) -> Ref<TextBlock> {
    mount(
        text_view(text, size, TEXT)
            .font_weight(FontWeight::Light)
            .letter_spacing(-0.9),
    )
}
fn strong(s: &str, size: f64, ink: &str) -> Ref<TextBlock> {
    mount(text_view(s, size, ink).font_weight(FontWeight::SemiBold))
}
fn kicker_view(s: &str, ink: &str) -> El<TextBlock> {
    text_view(s, 10.0, ink)
        .font_weight(FontWeight::SemiBold)
        .letter_spacing(1.65)
}
fn kicker(s: &str, ink: &str) -> Ref<TextBlock> {
    mount(kicker_view(s, ink))
}
fn surface_view(bg: &str, pad: f64, radius: f64) -> El<Border> {
    ui::border()
        .background(brush(bg))
        .corner_radius(CornerRadius::uniform(radius))
        .padding(Thickness::uniform(pad))
}
fn surface(bg: &str, pad: f64, radius: f64) -> Ref<Border> {
    mount(surface_view(bg, pad, radius))
}
fn stack(spacing: f64) -> Ref<StackPanel> {
    mount(ui::stack_panel().spacing(spacing))
}
fn row_view(spacing: f64) -> El<StackPanel> {
    ui::stack_panel()
        .spacing(spacing)
        .orientation(Orientation::Horizontal)
}
fn row(spacing: f64) -> Ref<StackPanel> {
    mount(row_view(spacing))
}
fn grid_view(cols: &str, rows: &str) -> El<Grid> {
    let mut g = ui::grid();
    if !cols.is_empty() {
        g = g.column_definitions(cols);
    }
    if !rows.is_empty() {
        g = g.row_definitions(rows);
    }
    g
}
fn grid(cols: &str, rows: &str) -> Ref<Grid> {
    mount(grid_view(cols, rows))
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
        "plus" => ("M11 3 V19 M3 11 H19", false),
        "up" => ("M5 14 L11 8 L17 14", false),
        "down" => ("M5 8 L11 14 L17 8", false),
        "remove" => ("M5 5 L17 17 M17 5 L5 17", false),
        "arrow" => ("M3 11 H19 M13 5 L19 11 L13 17", false),
        "back" => ("M19 11 H3 M9 5 L3 11 L9 17", false),
        "external" => ("M12 3 H20 V11 M20 3 L9 14 M8 5 H3 V20 H18 V15", false),
        "music" => (
            "M8 16 V5 L19 3 V13 M8 16 A3 2 0 1 1 2 16 A3 2 0 1 1 8 16 M19 13 A3 2 0 1 1 13 13 A3 2 0 1 1 19 13",
            false,
        ),
        "logo" => ("M2 13 L5 9 L8 15 L11 4 L14 18 L17 8 L20 12", false),
        _ => ("M3 11 H19", false),
    };
    let slot = ui::grid()
        .width(size)
        .height(size)
        .vertical_alignment(V::Center);
    let Ok(geometry) = Geometry::parse(path) else {
        return mount(slot);
    };
    let bounds = geometry.bounds();
    let scale = size / bounds.width.max(bounds.height);
    // Fit the path to its own aspect ratio, then center it in the square slot.
    let glyph = ui::path()
        .data(geometry)
        .width(bounds.width * scale)
        .height(bounds.height * scale)
        .stretch(Stretch::Uniform)
        .horizontal_alignment(H::Center)
        .vertical_alignment(V::Center);
    mount(slot.child(if fill {
        glyph.fill(brush(ink))
    } else {
        glyph.stroke(brush(ink)).stroke_thickness(1.65)
    }))
}
fn button_view(
    name: &str,
    content: impl IntoChild,
    primary: bool,
    f: impl Fn() + 'static,
) -> El<Button> {
    let light = LIGHT_PAGE.get();
    let click = Rc::new(f);
    ui::button()
        .child(content)
        .padding(Thickness::new(10.0, 6.0, 10.0, 6.0))
        .corner_radius(CornerRadius::uniform(0.0))
        .background(brush(if primary { ACCENT } else { "Transparent" }))
        .border_thickness(Thickness::uniform(0.0))
        .horizontal_content_alignment(H::Center)
        .vertical_content_alignment(V::Center)
        .automation_properties_name(name)
        .with(move |b| {
            if primary || light {
                let (hover, pressed) = if primary {
                    ("#FF85B7", "#D51F6F")
                } else {
                    ("#EFE5EE", "#F3D6E6")
                };
                b.resources()
                    .set("ButtonBackgroundPointerOver", Some(Rc::new(brush(hover))));
                b.resources()
                    .set("ButtonBackgroundPressed", Some(Rc::new(brush(pressed))));
            }
            let click = click.clone();
            b.click(move |_, _| click());
        })
}
fn button(name: &str, content: Ref<Control>, primary: bool, f: impl Fn() + 'static) -> Ref<Button> {
    mount(button_view(name, content, primary, f))
}
fn icon_button_view(
    name: &str,
    glyph: &str,
    ink: &str,
    size: f64,
    f: impl Fn() + 'static,
) -> El<Button> {
    button_view(name, icon(glyph, ink, size), false, f)
        .horizontal_alignment(H::Center)
        .vertical_alignment(V::Center)
        .padding(Thickness::uniform(0.0))
}
fn icon_button(
    name: &str,
    glyph: &str,
    ink: &str,
    size: f64,
    f: impl Fn() + 'static,
) -> Ref<Button> {
    mount(icon_button_view(name, glyph, ink, size, f))
}
fn action(name: &str, glyph: &str, ink: &str, code: &'static str, arg: &str) -> Ref<Button> {
    let arg = arg.to_owned();
    let b = icon_button(name, glyph, ink, 16.0, move || send(code, &arg));
    b.set_width(35.0);
    b.set_height(34.0);
    b
}
fn reveal_actions(host: &Ref<Border>, controls: &[Ref<Control>]) {
    let controls = controls.iter()
        .map(|control| (control.downgrade(), control.is_hit_test_visible()))
        .collect::<Vec<_>>();
    let keyboard_focus = Rc::new(Cell::new(false));
    let focus = keyboard_focus.clone();
    let host_ref = host.downgrade();
    let update = Rc::new(move || {
        let Some(host) = host_ref.upgrade() else { return };
        let visible = host.is_pointer_over() || focus.get();
        for (control, hit_test) in &controls {
            if let Some(control) = control.upgrade() {
                control.set_opacity(if visible { 1.0 } else { 0.0 });
                if *hit_test {
                    control.set_is_hit_test_visible(visible);
                }
            }
        }
    });
    update();
    let on_pointer = update.clone();
    host.add_property_changed_handler(Rc::new(move |change| {
        if change.property() == InputElement::is_pointer_over_property().as_property() {
            on_pointer();
        }
    }));
    let on_focus = update.clone();
    let focus = keyboard_focus.clone();
    host.add_handler(InputElement::got_focus_event(), move |_, event| {
        focus.set(matches!(event.navigation_method, NavigationMethod::Tab | NavigationMethod::Directional));
        on_focus();
    });
    host.add_handler(InputElement::lost_focus_event(), move |_, _| {
        keyboard_focus.set(false);
        update();
    });
}

fn collection_actions(item: &Item) -> Option<Ref<Border>> {
    if !matches!(item.kind.as_str(), "album" | "artist" | "playlist") {
        return None;
    }
    let buttons = row(2.0);
    let actions = [(format!("Play {}", item.title), "Play", "play", "play"),
        (format!("Add {} to queue", item.title), "Add to queue", "plus", "enqueue")];
    let count = if queueable_collection(item) { 2 } else { 1 };
    for (name, tip, glyph, code) in actions.into_iter().take(count) {
        let button = action(&name, glyph, "#FFFFFF", code, &item.id);
        button.set_width(30.0);
        button.set_height(30.0);
        for (key, ink) in [("ButtonBackgroundPointerOver", "#49364A"), ("ButtonBackgroundPressed", "#62364F")] {
            button.resources().set(key, Some(Rc::new(brush(ink))));
        }
        ToolTip::set_tip(&button, Some(Rc::new(tip.to_owned())));
        buttons.children().add(button.upcast::<Control>());
    }
    Some(mount(
        surface_view("#E01C1220", 0.0, 0.0)
            .horizontal_alignment(H::Left)
            .vertical_alignment(V::Top)
            .child(buttons),
    ))
}

fn playback_menu(
    host: &Control,
    play: (&'static str, &str),
    enqueue: Option<(&'static str, &str)>,
) {
    let items = std::iter::once(("Play", play.0, play.1))
        .chain(enqueue.map(|(action, arg)| ("Add to now playing", action, arg)))
        .map(|(label, action, arg)| {
            let arg = arg.to_owned();
            ui::menu_item()
                .header(Rc::new(label.to_owned()) as BoxedValue)
                .with(move |item| {
                    let arg = arg.clone();
                    item.click(move |_, _| send(action, &arg));
                })
        })
        .collect::<Vec<_>>();
    host.set_context_menu(Some(mount(ui::context_menu().children(items))));
}

fn queueable_collection(item: &Item) -> bool {
    matches!(item.kind.as_str(), "album" | "artist") || item.id.starts_with("local:playlist:")
}
fn pill(name: &str, glyph: &str, primary: bool, code: &'static str, arg: &str) -> Ref<Button> {
    let ink = if primary { "#220E19" } else { TEXT };
    let content = row_view(8.0).child(icon(glyph, ink, 14.0)).child(
        text_view(name, 12.0, ink)
            .font_weight(FontWeight::SemiBold)
            .vertical_alignment(V::Center),
    );
    let arg = arg.to_owned();
    let view = button_view(name, content, primary, move || send(code, &arg))
        .height(36.0)
        .padding(Thickness::new(14.0, 0.0, 14.0, 0.0));
    mount(if primary {
        view
    } else {
        view.background(brush(PANEL))
    })
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
    let frame = surface_view("#2A202B", 0.0, if external { 0.0 } else { radius })
        .height(height)
        .clip_to_bounds(true);
    if url.is_empty() {
        let glyph = icon("music", ACCENT, height.min(54.0) * 0.46);
        glyph.set_horizontal_alignment(H::Center);
        return mount(frame.child(glyph));
    }
    let image = mount(
        ui::image()
            .stretch(if external {
                Stretch::Uniform
            } else {
                Stretch::UniformToFill
            })
            .is_hit_test_visible(false),
    );
    let b = mount(frame.child(image.clone()));
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
    let template: Rc<dyn IControlTemplate> =
        FuncControlTemplate::for_type::<ScrollViewer>(|viewer, scope| {
            let layout = grid("*,Auto", "");
            layout.set_background(viewer.background());

            let presenter = mount(
                ui::scroll_content_presenter()
                    .name("PART_ContentPresenter")
                    .background(brush("Transparent"))
                    .clip_to_bounds(true),
            );
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

            let bar_template: Rc<dyn IControlTemplate> =
                FuncControlTemplate::for_type::<ScrollBar>(|_, scope| {
                    let track = mount(
                        ui::track()
                            .orientation(Orientation::Vertical)
                            .is_direction_reversed(true),
                    );
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

                    let thumb = mount(
                        ui::thumb()
                            .name("VerticalThumb")
                            .min_height(16.0)
                            .template(FuncControlTemplate::new(|_, _| {
                                surface("#B3B3B3", 0.0, 0.0).upcast()
                            }) as Rc<dyn IControlTemplate>),
                    );
                    track.set_thumb(thumb.register_in_name_scope(&**scope));
                    track.set_decrease_button(
                        scroll_repeat("PART_PageUpButton", None).register_in_name_scope(&**scope),
                    );
                    track.set_increase_button(
                        scroll_repeat("PART_PageDownButton", None).register_in_name_scope(&**scope),
                    );

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
                    mount(
                        surface_view("Transparent", 0.0, 0.0)
                            .border_brush(brush("#A6A6A6"))
                            .border_thickness(Thickness::new(0.0, 0.0, 1.0, 0.0))
                            .child(controls),
                    )
                    .upcast()
                });
            let bar = mount(
                ui::scroll_bar()
                    .name("PART_VerticalScrollBar")
                    .orientation(Orientation::Vertical)
                    .with(|bar| bar.set_is_tab_stop(false))
                    .min_width(0.0)
                    .width(10.0)
                    .margin(Thickness::new(6.0, 0.0, 0.0, 0.0))
                    .template(bar_template),
            );
            put(&layout, 1, 0, bar.register_in_name_scope(&**scope).upcast());
            layout.upcast()
        });
    mount(
        ui::scroll_viewer()
            .horizontal_scroll_bar_visibility(ScrollBarVisibility::Disabled)
            .vertical_scroll_bar_visibility(ScrollBarVisibility::Auto)
            .template(template),
    )
}
fn scroll_repeat(name: &str, arrow_up: Option<bool>) -> Ref<RepeatButton> {
    let template: Rc<dyn IControlTemplate> = FuncControlTemplate::new(move |_, _| {
        let hit_area = surface_view("Transparent", 0.0, 0.0);
        let hit_area = if let Some(up) = arrow_up {
            hit_area.child(
                ui::path()
                    .data(
                        Geometry::parse(if up {
                            "M0 3 L2.5 0 L5 3 Z"
                        } else {
                            "M0 0 L5 0 L2.5 3 Z"
                        })
                        .ok(),
                    )
                    .width(5.0)
                    .height(3.0)
                    .stretch(Stretch::Uniform)
                    .fill(brush("#858585"))
                    .horizontal_alignment(H::Center)
                    .vertical_alignment(V::Center),
            )
        } else {
            hit_area
        };
        mount(hit_area).upcast()
    });
    mount(
        ui::repeat_button()
            .name(name)
            .focusable(false)
            .min_width(0.0)
            .min_height(0.0)
            .horizontal_alignment(H::Stretch)
            .vertical_alignment(V::Stretch)
            .template(template),
    )
}
fn add_reveal_light(root: &Grid, rows: i32) -> WeakRef<Border> {
    // Paint over the page backgrounds and clip at the app's outer edge.
    let lighting = mount(ui::border().is_hit_test_visible(false).grid_row_span(rows));
    put(root, 0, 0, lighting.clone().upcast());
    effects::reveal_light(root, &lighting)
}

fn build() {
    let (width, host, model) = UI.with(|s| {
        let s = s.borrow();
        (s.width, s.shell.clone().unwrap(), s.model.clone())
    });
    let mobile = width < 790.0;
    let main = scroll();
    let body = mount(surface_view(BG, 0.0, 0.0).clip_to_bounds(true).grid_row(1));
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
    let chrome = mount(surface_view(BG, 0.0, 0.0).child(top_bar(mobile, &model)));
    let footer = mount(surface_view(BG, 0.0, 0.0).background(None::<Rc<dyn IBrush>>));
    let backdrop = mount(
        surface_view("#01000000", 0.0, 0.0)
            .is_hit_test_visible(false)
            .vertical_alignment(V::Bottom)
            .height(74.0),
    );
    let transport = mount(
        surface_view(BG, 0.0, 0.0)
            .grid_row(2)
            .child(ui::grid().children((backdrop.clone(), footer.clone()))),
    );
    let root = mount(
        grid_view(
            "",
            if mobile {
                "99,*,77"
            } else if cfg!(target_os = "emscripten") {
                "76,*,86"
            } else {
                "48,*,86"
            },
        )
        .clip_to_bounds(true)
        .children((chrome.clone(), body.clone(), transport.clone())),
    );

    let _reveal = (!UI.with(|ui| ui.borrow().reduced)).then(|| add_reveal_light(&root, 3));

    UI.with(|s| {
        let mut s = s.borrow_mut();
        #[cfg(target_os = "emscripten")]
        {
            s.reveal = _reveal;
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

fn back_button(width: f64, enabled: bool) -> Ref<Button> {
    let back = action("Back", "back", if enabled { TEXT } else { MUTED }, "back", "");
    back.set_width(width);
    back.set_is_enabled(enabled);
    ToolTip::set_tip(&back, Some(Rc::new("Back".to_owned())));
    back
}

fn top_bar(mobile: bool, model: &Model) -> Ref<Control> {
    let compact_desktop = mobile && cfg!(not(target_os = "emscripten"));
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
    stripe.set_height(if cfg!(target_os = "emscripten") { 50.0 } else { 32.0 });
    put(&top, 0, 0, stripe.upcast());

    let g = grid(
        if mobile { "*,Auto" } else { "Auto,*,Auto" },
        if mobile && !compact_desktop { "49,42" } else { "" },
    );
    g.set_margin(Thickness::new(
        if mobile { 7.0 } else { 20.0 },
        0.0,
        15.0,
        0.0,
    ));
    let navigation = row(if mobile { 0.0 } else { 5.0 });
    center(&navigation);
    let back = surface("Transparent", 0.0, 0.0);
    center(&back);
    back.set_width(if mobile { 26.0 } else { 32.0 });
    back.set_child(back_button(back.width(), model.can_go_back));
    navigation.children().add(back.clone().upcast::<Control>());
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
    #[cfg(not(target_os = "emscripten"))]
    WindowDecorationProperties::set_element_role(&navigation, WindowDecorationsElementRole::User);
    put(&g, 0, 0, navigation.upcast());

    let search = mount(
        ui::text_box()
            .min_width(0.0)
            .max_width(if mobile { 560.0 } else { 285.0 })
            .height(32.0)
            .placeholder_text("Search music")
            .text(model.query.as_str())
            .background(brush("#211923"))
            .corner_radius(CornerRadius::uniform(0.0))
            .inner_left_content(Control::boxed(icon("search", MUTED, 13.0)))
            .vertical_alignment(V::Center)
            .automation_properties_name("Search songs, artists, albums"),
    );
    let weak = search.downgrade();
    search.text_changed(move |_, _| {
        if let Some(search) = weak.upgrade() {
            send("query", &search.text().unwrap_or_default());
        }
    });
    if mobile && !compact_desktop {
        Grid::set_column_span(&search, 2);
        put(&g, 0, 1, search.clone().upcast());
    } else if !mobile {
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
    #[cfg(not(target_os = "emscripten"))]
    {
        WindowDecorationProperties::set_element_role(&search, WindowDecorationsElementRole::User);
        WindowDecorationProperties::set_element_role(&tools, WindowDecorationsElementRole::User);
    }
    put(&g, if mobile { 1 } else { 2 }, 0, tools.upcast());
    #[cfg(target_os = "emscripten")]
    let header = {
        put(&top, 1, 0, g.upcast());
        top.upcast()
    };
    #[cfg(not(target_os = "emscripten"))]
    let header = {
        let caption = surface("#00000000", 0.0, 0.0);
        caption.set_child(top.clone());
        UI.with(|ui| ui.borrow_mut().caption_area = Some(caption.downgrade()));
        if compact_desktop {
            search.set_margin(Thickness::new(7.0, 0.0, 28.0, 0.0));
            put(&top, 1, 0, search.clone().upcast());
            let compact = grid("", "48,*");
            put(&compact, 0, 0, caption.upcast());
            put(&compact, 0, 1, g.upcast());
            compact.upcast()
        } else {
            put(&top, 1, 0, g.upcast());
            caption.upcast()
        }
    };
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        ui.search = Some(search);
        ui.nav = labels;
        ui.back = Some((back, (LIGHT_PAGE.get(), model.can_go_back)));
    });
    header
}
fn header(title: &str, detail: &str) -> Ref<Control> {
    mount(
        grid_view("*,Auto", "").children((
            text_view(title, 23.0, TEXT).font_weight(FontWeight::SemiBold),
            kicker_view(detail, DIM)
                .vertical_alignment(V::Center)
                .grid_column(1),
        )),
    )
    .upcast()
}
fn artwork_tile(item: &Item, size: f64, play: bool) -> Ref<Control> {
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
    if let Some(actions) = collection_actions(item) {
        let layers = grid("", "");
        put(&layers, 0, 0, tile.upcast());
        actions.set_vertical_alignment(V::Bottom);
        put(&layers, 0, 0, actions.clone().upcast());
        let host = surface("Transparent", 0.0, 0.0);
        host.set_child(layers);
        let enqueue = queueable_collection(item).then_some(("enqueue", item.id.as_str()));
        playback_menu(&host, ("play", &item.id), enqueue);
        reveal_actions(&host, &[actions.upcast()]);
        host.upcast()
    } else {
        tile.upcast()
    }
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
    let liked = stack(8.0);
    liked.children()
        .add(metro_title("liked songs", 42.0).upcast::<Control>());
    if m.liked_music.is_empty() {
        let note = label("Songs you like appear here.", 12.0, MUTED);
        note.set_text_wrapping(TextWrapping::Wrap);
        liked.children().add(note.upcast::<Control>());
    } else {
        liked.children().add(tile_rows(&m.liked_music, 2, tiles * 0.74));
        let b = pill("All liked songs", "heart", false, "navigate", "liked");
        b.set_horizontal_alignment(H::Left);
        liked.children().add(b.upcast::<Control>());
    }
    let new_music = stack(8.0);
    new_music
        .children()
        .add(metro_title("new", 42.0).upcast::<Control>());
    if let Some(featured) = m.collections.first() {
        let mosaic = row(5.0);
        mosaic
            .children()
            .add(artwork_tile(featured, tiles * 1.35, false));
        mosaic.children().add(tile_rows(
            &m.collections[1..m.collections.len().min(5)],
            2,
            tiles * 0.65,
        ));
        new_music.children().add(mosaic.upcast::<Control>());
        let play = pill("Play", "play", true, "play", &featured.id);
        play.set_horizontal_alignment(H::Left);
        new_music.children().add(play.upcast::<Control>());
    }
    if m.collections.is_empty() {
        let import = pill("Open local music", "library", true, "navigate", "local");
        import.set_horizontal_alignment(H::Left);
        new_music.children().add(import.upcast::<Control>());
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
    for (index, section) in [liked, new_music, history].into_iter().enumerate() {
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
                .add(artwork_tile(item, size, item.kind == "track"));
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
    let layout = StackLayout::new();
    layout.set_spacing(spacing);
    let template = FuncDataTemplate::for_type::<T>(
        move |item, _| {
            let previous = LIGHT_PAGE.replace(light);
            let control = build(item);
            LIGHT_PAGE.set(previous);
            Some(control)
        },
        false,
    );
    mount(
        ui::items_repeater()
            .layout(layout.upcast::<ferroui_controls_repeater::Layout>())
            .with(move |repeater| repeater.set_data_template(Some(template.clone()))),
    )
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
        if let Some(actions) = collection_actions(item) {
            let layers = grid("", "");
            put(&layers, 0, 0, b.upcast());
            actions.set_margin(Thickness::new(8.0, size - 22.0, 0.0, 0.0));
            put(&layers, 0, 0, actions.clone().upcast());
            light.set_child(layers);
            let enqueue = queueable_collection(item).then_some(("enqueue", item.id.as_str()));
            playback_menu(&light, ("play", &item.id), enqueue);
            reveal_actions(&light, &[actions.upcast()]);
        } else {
            light.set_child(b);
        }
        card.set_child(light);
        put(&g, idx as i32, 0, card.upcast());
    }
    g.upcast()
}
fn track(item: &Item, index: usize, compact: bool) -> Ref<Control> {
    track_row(item, index, compact, None)
}

fn track_row(
    item: &Item,
    index: usize,
    compact: bool,
    editable_queue: Option<usize>,
) -> Ref<Control> {
    let playlist_action = cfg!(target_os = "emscripten") && item.id.starts_with("local:track:");
    let light = surface("Transparent", 0.0, 0.0);
    reveal(&light);
    let g = grid(
        if editable_queue.is_some() {
            "*,35,29,29,29"
        } else if compact {
            "*,35"
        } else if item.id.starts_with("spotify:") || playlist_action {
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
    let artwork = grid("", "");
    put(&artwork, 0, 0, image.upcast());
    let play_glyph = surface("#B5161018", 0.0, 0.0);
    play_glyph.set_child(icon("play", "#FFFFFF", 14.0));
    play_glyph.set_is_hit_test_visible(false);
    put(&artwork, 0, 0, play_glyph.clone().upcast());
    put(&r, col, 0, artwork.upcast());
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
    let queue_entry = format!("{index}:{}", item.id);
    let (code, arg) = if editable_queue.is_some() {
        ("queue-play", queue_entry.clone())
    } else {
        ("play", item.id.clone())
    };
    playback_menu(&light, (code, &arg), (!compact).then_some(("enqueue", item.id.as_str())));
    let b = button(
        &format!("Play {}", item.title),
        r.upcast(),
        false,
        move || send(code, &arg),
    );
    b.set_horizontal_alignment(H::Stretch);
    b.set_horizontal_content_alignment(H::Stretch);
    b.set_padding(Thickness::new(6.0, 4.0, 4.0, 4.0));
    put(&g, 0, 0, b.upcast());
    let mut hover = vec![play_glyph.upcast::<Control>()];
    if compact {
        let t = label(&time(item.duration), 10.0, DIM);
        center(&t);
        put(&g, 1, 0, t.upcast());
        if let Some(count) = editable_queue {
            for (column, name, glyph, code, enabled) in [
                (2, format!("Move {} up in queue", item.title), "up", "queue-up", index > 0),
                (3, format!("Move {} down in queue", item.title), "down", "queue-down", index + 1 < count),
                (4, format!("Remove {} from queue", item.title), "remove", "queue-remove", true),
            ] {
                let button = action(&name, glyph, MUTED, code, &queue_entry);
                button.set_width(29.0);
                button.set_is_enabled(enabled);
                ToolTip::set_tip(&button, Some(Rc::new(name)));
                hover.push(button.clone().upcast());
                put(&g, column, 0, button.upcast());
            }
        }
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
        let add = action("Add to queue", "plus", MUTED, "enqueue", &item.id);
        ToolTip::set_tip(&add, Some(Rc::new("Add to queue".to_owned())));
        hover.push(add.clone().upcast());
        put(&g, 2, 0, add.upcast());
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
        } else if playlist_action {
            let add = action(
                "Add to playlist",
                "library",
                MUTED,
                "local-playlist",
                &item.id,
            );
            ToolTip::set_tip(&add, Some(Rc::new("Add to playlist".to_owned())));
            put(&g, 3, 0, add.upcast());
            4
        } else {
            3
        };
        put(&g, col, 0, t.upcast());
    }
    light.set_child(g);
    reveal_actions(&light, &hover);
    light.upcast()
}
fn main_page(m: &Model, width: f64, animate: bool) -> Ref<Control> {
    if m.page == "home" {
        return quickplay(m, width, animate);
    }
    if matches!(m.page.as_str(), "queue" | "local-queue") {
        return now_playing(m, width, animate);
    }
    let mobile = width < 790.0;
    let page = stack(if mobile { 12.0 } else { 15.0 });
    page.set_margin(Thickness::uniform(if mobile { 12.0 } else { 20.0 }));
    #[cfg(not(target_os = "emscripten"))]
    if m.page == "account" {
        page.children().add(header("Spotify account", ""));
        let explanation = if m.connected {
            "Spotify is connected. Use Playback devices to change where music plays."
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
            let input = mount(
                ui::text_box()
                    .max_width(460.0)
                    .horizontal_alignment(H::Left)
                    .placeholder_text("Spotify developer Client ID")
                    .automation_properties_name("Spotify developer Client ID")
                    .text(m.client_id.as_str()),
            );
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
                if [
                    "spotify:album:",
                    "spotify:artist:",
                    "local:album:",
                    "local:playlist:",
                ]
                .iter()
                .any(|prefix| m.detail.starts_with(prefix))
                {
                    actions.children().add(
                        pill("Add to queue", "plus", false, "enqueue", &m.detail).upcast::<Control>(),
                    );
                }
            }
            #[cfg(target_os = "emscripten")]
            if m.detail.starts_with("local:playlist:") {
                actions.children().add(
                    pill(
                        "Edit playlist",
                        "library",
                        false,
                        "local-playlist",
                        &m.detail,
                    )
                    .upcast::<Control>(),
                );
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
            if m.detail.starts_with("local:album:") && m.items.iter().all(|item| item.art.is_empty()) {
                actions.children().add(pill("Find cover art", "search", false, "local-cover", &m.detail).upcast::<Control>());
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
    let tracks = m.items.iter().filter(|item| item.kind.is_empty() || item.kind == "track");
    let frame = surface(
        if LIGHT_PAGE.get() {
            "#FCFAFC"
        } else {
            "Transparent"
        },
        0.0,
        0.0,
    );
    if bounded_catalog(&m.page) {
        #[derive(PartialEq)]
        enum Row { Cards(Vec<Item>), Track(usize, Item) }
        let rows = cards.chunks(cols).map(|chunk| Row::Cards(chunk.to_vec()))
            .chain(tracks.cloned().enumerate().map(|(index, item)| Row::Track(index, item)));
        let list = virtual_list(rows, 2.0, move |row| match row {
            Row::Cards(items) => collection_card_row(items, cols),
            Row::Track(index, item) => track(item, *index, false),
        });
        let viewer = scroll();
        viewer.set_is_scroll_chaining_enabled(false);
        AutomationProperties::set_name(&viewer, Some("Music results"));
        viewer.set_content(Some(Rc::new(list)));
        page.set_margin(Thickness::default());
        let layout = grid("", "Auto,*");
        layout.set_margin(Thickness::uniform(if mobile { 12.0 } else { 20.0 }));
        put(&layout, 0, 0, page.upcast());
        put(&layout, 0, 1, viewer.upcast());
        frame.set_child(layout);
    } else {
        if !cards.is_empty() {
            page.children().add(collection_cards(&cards, cols));
        }
        page.children().add(track_list(tracks, false));
        frame.set_child(page);
    }
    if animate {
        effects::entrance(&frame, 20.0, 0);
    }
    frame.upcast()
}

fn bounded_catalog(page: &str) -> bool {
    matches!(page, "detail" | "liked" | "search")
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
        Grid::set_row(&viewer, 1);

        let repeater = virtual_repeater(spacing, build);
        viewer.set_content(Some(Rc::new(repeater.clone().upcast::<Control>())));

        let host = mount(grid_view("", "Auto,*").children((
            kicker_view(title, TEXT).margin(Thickness::new(0.0, 0.0, 0.0, 8.0)),
            viewer.clone(),
        )));
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
    status: Ref<TextBlock>,
    artists: CollectionPane<Option<String>>,
    playlists: CollectionPane<Vec<Item>>,
    songs: CollectionPane<(usize, Item)>,
    playlist_empty: Ref<TextBlock>,
    song_empty: Ref<TextBlock>,
    tabs: Vec<Ref<TextBlock>>,
    selected_artist: RefCell<Option<String>>,
    cols: usize,
    local: bool,
    #[cfg(target_os = "emscripten")]
    folder_status: Option<Ref<TextBlock>>,
    #[cfg(target_os = "emscripten")]
    rescan: Option<Ref<Button>>,
}

impl CollectionView {
    fn new(m: &Model, width: f64, animate: bool) -> Self {
        let local = m.page == "local";
        let compact = width < 790.0;
        let cols = if width >= 1360.0 {
            4
        } else if width >= 1050.0 {
            3
        } else {
            2
        };
        let inset = if compact { 16.0 } else { 44.0 };
        let root = grid_view("", "Auto,Auto,Auto,Auto,Auto,*")
            .margin(Thickness::new(inset, 16.0, inset, 16.0))
            .max_width(1370.0);

        let heading = surface("Transparent", 0.0, 0.0);
        heading.set_horizontal_alignment(H::Left);
        heading.set_child(metro_title(if local { "local music" } else { "music" }, if compact { 34.0 } else { 44.0 }));
        heading.set_margin(Thickness::new(
            0.0,
            0.0,
            0.0,
            if compact { 10.0 } else { 20.0 },
        ));
        if animate {
            effects::entrance(&heading, 24.0, 0);
        }
        let root = root.child(heading);

        let mut filters = ui::wrap_panel()
            .grid_row(1)
            .item_spacing(3.0)
            .line_spacing(3.0)
            .margin(Thickness::new(
                0.0,
                0.0,
                0.0,
                if compact { 10.0 } else { 20.0 },
            ));
        filters = if local {
            let filters =
                filters.child(pill("Collection", "library", false, "navigate", "library"));
            #[cfg(not(target_os = "emscripten"))]
            let filters = filters.child(pill(
                "Choose music folder",
                "library",
                true,
                "import-folder",
                "",
            ));
            #[cfg(target_os = "emscripten")]
            let filters = filters.children((
                pill("New playlist", "plus", false, "local-playlist", ""),
                pill("Local queue", "queue", false, "navigate", "local-queue"),
            ));
            filters
        } else {
            filters.children((
                pill("Liked songs", "heart", false, "navigate", "liked"),
                pill("Local music", "library", false, "navigate", "local"),
            ))
        };
        if m.connected && !local {
            for (name, id) in [("Artists", "section:artists"), ("Albums", "section:albums")] {
                filters = filters.child(pill(name, "library", false, "open", id));
            }
        }
        let root = root.child(filters);

        #[cfg(target_os = "emscripten")]
        let (root, folder_status, rescan) = if local {
            let description = mount(text_view("", 12.0, MUTED).text_wrapping(TextWrapping::Wrap));
            let rescan = pill("Rescan folder", "library", false, "rescan-folder", "");
            let panel = surface_view("#F9E8F1", 12.0, 0.0)
                .grid_row(2)
                .border_brush(brush("#C23B80"))
                .border_thickness(Thickness::uniform(1.0))
                .margin(Thickness::new(0.0, 0.0, 0.0, 10.0))
                .child(
                    ui::stack_panel().spacing(5.0).children((
                        text_view("Drop your music folder here", 17.0, TEXT)
                            .font_weight(FontWeight::SemiBold),
                        description.clone(),
                        ui::wrap_panel()
                            .item_spacing(4.0)
                            .line_spacing(4.0)
                            .children((
                                pill("Choose music folder", "library", false, "import-folder", ""),
                                rescan.clone(),
                            )),
                    )),
                );
            (root.child(panel), Some(description), Some(rescan))
        } else {
            (root, None, None)
        };

        let artists = CollectionPane::new("ARTISTS", 1.0, artist_button);
        let playlists = CollectionPane::new(
            if local && cfg!(target_os = "emscripten") {
                "ALBUMS & PLAYLISTS"
            } else if local {
                "ALBUMS"
            } else {
                "PLAYLISTS"
            },
            8.0,
            move |chunk: &Vec<Item>| collection_card_row(chunk, cols),
        );
        let songs = CollectionPane::new("SONGS", 2.0, |(index, item)| track(item, *index, false));
        let playlist_empty = collection_empty(&playlists.host, if local { "Tagged albums appear here." } else { "Your playlists appear here." });
        let song_empty = collection_empty(&songs.host, "No songs to show.");

        let catalog = grid(if compact { "" } else { "1.05*,2.1*,1.8*" }, "");
        let mut tabs = Vec::new();
        let tab_row = row(8.0);
        for (index, (name, pane)) in [
            ("Artists", &artists.host),
            (if local { "Albums" } else { "Playlists" }, &playlists.host),
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
        let root = if compact {
            tab_row.set_margin(Thickness::new(0.0, 0.0, 0.0, 10.0));
            Grid::set_row(&tab_row, 3);
            root.child(tab_row)
        } else {
            root
        };
        let status = mount(
            text_view("", 13.0, "#BA1267")
                .text_wrapping(TextWrapping::Wrap)
                .margin(Thickness::new(0.0, 0.0, 0.0, 10.0))
                .grid_row(4),
        );
        Grid::set_row(&catalog, 5);
        let root = mount(root.children((status.clone(), catalog)));

        let view = Self {
            root,
            status,
            artists,
            playlists,
            songs,
            playlist_empty,
            song_empty,
            tabs,
            selected_artist: RefCell::default(),
            cols,
            local,
            #[cfg(target_os = "emscripten")]
            folder_status,
            #[cfg(target_os = "emscripten")]
            rescan,
        };
        view.show_pane(if local && m.collections.is_empty() { 2 } else { 1 });
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
        #[cfg(target_os = "emscripten")]
        if let (Some(label), Some(rescan)) = (&self.folder_status, &self.rescan) {
            let text = if m.folder_name.is_empty() {
                "Drag a folder from Finder or your file manager to import its tracks and subfolders.".to_owned()
            } else if m.folder_notice.is_empty() {
                format!("Music folder: {}", m.folder_name)
            } else {
                format!("{} · {}", m.folder_name, m.folder_notice)
            };
            label.set_text(Some(&text));
            rescan.set_is_visible(m.folder_can_rescan);
        }
        self.status.set_text(Some(&m.status));
        self.status.set_is_visible(!m.status.is_empty());
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
    let Some(artist) = artist.as_deref() else { return button.upcast() };
    let play = action(&format!("Play {artist}"), "play", TEXT, "artist-play", artist);
    let add = action(&format!("Add {artist} to queue"), "plus", MUTED, "artist-enqueue", artist);
    let actions = row(0.0);
    for (control, tip) in [(&play, "Play"), (&add, "Add to queue")] {
        control.set_width(27.0);
        control.set_height(26.0);
        ToolTip::set_tip(control, Some(Rc::new(tip.to_owned())));
        actions.children().add(control.clone().upcast::<Control>());
    }
    let layout = grid("*,Auto", "");
    put(&layout, 0, 0, button.upcast());
    put(&layout, 1, 0, actions.clone().upcast());
    let host = surface("Transparent", 0.0, 0.0);
    host.set_child(layout);
    playback_menu(&host, ("artist-play", artist), Some(("artist-enqueue", artist)));
    reveal_actions(&host, &[actions.upcast()]);
    host.upcast()
}

struct WallArt {
    covers: Vec<String>,
    next: usize,
    random: u64,
}

impl WallArt {
    fn new(covers: Vec<String>) -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |time| time.as_nanos() as u64);
        let next = covers.len();
        Self {
            covers,
            next,
            random: seed.max(1),
        }
    }

    fn next_cover(&mut self) -> &str {
        if self.next == self.covers.len() {
            for index in (1..self.covers.len()).rev() {
                self.random ^= self.random << 13;
                self.random ^= self.random >> 7;
                self.random ^= self.random << 17;
                self.covers
                    .swap(index, (self.random % (index as u64 + 1)) as usize);
            }
            self.next = 0;
        }
        let index = self.next;
        self.next += 1;
        &self.covers[index]
    }
}

fn fill_art_wall(wall: &Grid, columns: usize, art: &mut WallArt) {
    wall.children().clear();
    for row in 0..3 {
        for column in 0..columns {
            let cell = cover(art.next_cover(), f64::NAN, 0.0);
            cell.set_margin(Thickness::uniform(1.0));
            put(wall, column as i32, row, cell.upcast());
        }
    }
}

fn now_playing(m: &Model, width: f64, animate: bool) -> Ref<Control> {
    let mobile = width < 790.0;
    let stage = grid("", "");
    stage.set_clip_to_bounds(true);
    if !mobile {
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
        .collection_art
        .iter()
        .chain(
            m.collections
                .iter()
                .chain(&m.queue)
                .chain(&m.recent)
                .chain(std::iter::once(&m.current))
                .map(|item| &item.art),
        )
        .filter(|art| !art.is_empty() && seen.insert(art.as_str()))
        .cloned()
        .collect();
    if !covers.is_empty() {
        let columns = if mobile { 4 } else { 8 };
        let wall = grid(&vec!["*"; columns].join(","), "*,*,*");
        wall.set_is_hit_test_visible(false);
        let mut art = WallArt::new(covers);
        fill_art_wall(&wall, columns, &mut art);
        if art.covers.len() > 1 && !UI.with(|ui| ui.borrow().reduced) {
            use ferroui_base::threading::{DispatcherPriority, DispatcherTimer};
            let weak = wall.downgrade();
            let art = RefCell::new(art);
            DispatcherTimer::run(
                move || {
                    let Some(wall) = weak
                        .upgrade()
                        .filter(|wall| wall.is_attached_to_visual_tree())
                    else {
                        return false;
                    };
                    fill_art_wall(&wall, columns, &mut art.borrow_mut());
                    true
                },
                std::time::Duration::from_secs(10),
                DispatcherPriority::NORMAL,
            )
            .detach();
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
    let p = grid("", "Auto,*");
    let q = grid("*,Auto", "");
    put(
        &q,
        0,
        0,
        strong(
            if m.page == "local-queue" {
                "Local queue"
            } else {
                "Up next"
            },
            15.0,
            TEXT,
        )
        .upcast(),
    );
    let editable_queue =
        m.page == "local-queue" || !m.connected || m.current.id.starts_with("local:");
    if !m.queue.is_empty() && editable_queue {
        put(&q, 1, 0, pill("Clear queue", "remove", false, "queue-clear", "").upcast());
    } else if !m.queue.is_empty() {
        put(
            &q,
            1,
            0,
            kicker(&format!("{:02} TRACKS", m.queue.len()), DIM).upcast(),
        );
    }
    q.set_margin(Thickness::new(0.0, 0.0, 0.0, 10.0));
    put(&p, 0, 0, q.upcast());
    if m.queue.is_empty() {
        put(&p, 0, 1, label("Your queue is empty.", 12.0, DIM).upcast());
    } else {
        let editable = editable_queue.then_some(m.queue.len());
        let list = scroll();
        list.set_is_scroll_chaining_enabled(false);
        AutomationProperties::set_name(&list, Some("Up next tracks"));
        list.set_content(Some(Rc::new(virtual_list(
            m.queue.iter().cloned().enumerate(), 2.0,
            move |(index, item)| track_row(item, *index, true, editable),
        ))));
        put(&p, 0, 1, list.upcast());
    }
    let page = grid("", if mobile { "3*,2*" } else { "*" });
    put(&page, 0, 0, stage.clone().upcast());
    let queue = surface(if mobile { BG } else { "#D51B121D" }, 13.0, 0.0);
    queue.set_child(p);
    if mobile {
        put(&page, 0, 1, queue.upcast());
    } else {
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
    let s = mount(
        ui::slider()
            .maximum(max.max(1.0))
            .min_width(0.0)
            .height(25.0)
            .automation_properties_name(name)
            .with(move |slider| slider.set_range_value(value)),
    );
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
fn player(m: &Model, width: f64, fire: Option<&effects::TransportFire>) -> Ref<Control> {
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
    let art = cover(&m.current.art, size - 2.0, 0.0);
    let frame = surface("Transparent", 0.0, 0.0);
    frame.set_child(art);
    frame.set_width(size);
    frame.set_height(size);
    frame.set_border_brush(Some(brush(MUTED)));
    frame.set_border_thickness(Thickness::uniform(1.0));
    frame.set_horizontal_alignment(H::Left);
    center(&frame);
    let identity = grid(if compact { "38,*" } else { "49,*" }, "");
    put(&identity, 0, 0, frame.upcast());
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
    let toggle = mount(
        icon_button_view(
            if m.playing { "Pause" } else { "Play" },
            if m.playing { "pause" } else { "play" },
            TEXT,
            14.0,
            || send("toggle", ""),
        )
        .width(36.0)
        .height(36.0)
        // The triangle's center of area sits left of its bounding-box center.
        .padding(Thickness::new(
            if m.playing { 0.0 } else { 4.0 },
            0.0,
            0.0,
            0.0,
        ))
        .corner_radius(CornerRadius::uniform(18.0))
        .border_brush(brush(MUTED))
        .border_thickness(Thickness::uniform(1.5)),
    );
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
    let meter = row(2.0);
    meter.set_margin(Thickness::new(5.0, 0.0, 0.0, 0.0));
    meter.set_is_hit_test_visible(false);
    center(&meter);
    let bars = std::array::from_fn(|index| {
        let bar = surface(ACCENT, 0.0, 0.0);
        bar.set_width(3.0);
        bar.set_height(13.0);
        bar.set_render_transform_origin(RelativePoint::new(0.5, 1.0, RelativeUnit::Relative));
        let scale = ScaleTransform::new();
        scale.set_scale_y([0.26, 0.62, 1.0, 0.42][index]);
        bar.set_render_transform(Some(scale.clone().into()));
        meter.children().add(bar.upcast::<Control>());
        scale
    });
    if let Some(fire) = fire {
        fire.set_meter(bars);
    }
    buttons.children().add(meter.upcast::<Control>());
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
        let _was_collection = LIGHT_PAGE.replace(collection);
        // Recreate only this button when its state changes so browser accessibility
        // publishes the enabled state as well as the new icon color.
        let back = UI.with(|ui| {
            let mut ui = ui.borrow_mut();
            let (back, state) = ui.back.as_mut()?;
            let next = (collection, m.can_go_back);
            (std::mem::replace(state, next) != next).then(|| back.clone())
        });
        if let Some(back) = back {
            back.set_child(back_button(back.width(), m.can_go_back));
        }
        #[cfg(not(target_os = "emscripten"))]
        if _was_collection != collection && let Some(app) = Application::current() {
            app.resources()
                .set("WindowCaptionForeground", Some(Rc::new(brush(TEXT))));
        }
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
            if matches!(m.page.as_str(), "library" | "local") {
                let existing = UI.with(|ui| ui.borrow().collection.clone())
                    .filter(|view| view.local == (m.page == "local"));
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
            } else if bounded_catalog(&m.page) || matches!(m.page.as_str(), "queue" | "local-queue")
            {
                UI.with(|ui| ui.borrow_mut().collection = None);
                if let Some(main) = &main {
                    main.set_content(None);
                }
                host.set_child(main_page(&m, width, animate && !reduced));
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
            footer.set_child(player(&m, width, fire.as_deref()));
            LIGHT_PAGE.set(false);
        }
        if let Some(fire) = fire {
            fire.set_playing(m.playing);
        }
    }
}

fn is_collection(page: &str) -> bool {
    matches!(page, "library" | "local" | "liked" | "detail")
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
        #[cfg(target_os = "emscripten")]
        let folder_changed = previous.folder_name != model.folder_name
            || previous.folder_notice != model.folder_notice
            || previous.folder_can_rescan != model.folder_can_rescan;
        #[cfg(not(target_os = "emscripten"))]
        let folder_changed = false;
        let connected_changed = previous.connected != model.connected;
        let body = body
            && (reset
                || match model.page.as_str() {
                    "library" | "local" => {
                        folder_changed
                            || previous.page != model.page
                            || previous.items != model.items
                            || previous.collections != model.collections
                            || previous.status != model.status
                    }
                    "home" => {
                        previous.page != model.page
                            || previous.items != model.items
                            || previous.collections != model.collections
                            || previous.liked_music != model.liked_music
                            || previous.recent != model.recent
                            || previous.status != model.status
                    }
                    "queue" | "local-queue" => {
                        previous.page != model.page
                            || previous.collection_art != model.collection_art
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

#[cfg_attr(target_os = "emscripten", wasm_bindgen(js_name = setSearchText))]
pub fn set_search_text(value: &str) {
    if let Some(search) = UI.with(|ui| ui.borrow().search.clone()) {
        search.set_text(Some(value));
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
    let mut stream = Cursor::new(bytes);
    let bitmap = if url.starts_with("local-cover:") {
        Bitmap::decode_to_width(&mut stream, 320, ferroui_base::media::imaging::BitmapInterpolationMode::HighQuality)
    } else {
        Bitmap::from_stream(&mut stream)
    };
    if let Ok(bitmap) = bitmap {
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
