This project uses qmetaobject-rs to use Rust with QML. 

UI related files are in src/ui/ folder

Inspect the existing implementation before proposing a new API. Read the relevant component, backend, shared base class, and similar features first. Reuse existing APIs and components such as ClosingHandler, QmlUtils.copy_to_clipboard, IconToolButton, and device services instead of introducing a parallel implementation without checking what already exists.

For refresh actions, use src/ui/base/IconToolButton.qml with qrc:/resources/icons/ic_outline-refresh.svg and a translated Refresh tooltip and accessible name instead of a text button or a custom refresh control.

For directory selection UI, reuse src/ui/base/LocationSelector.qml instead of creating a separate path label and Change button. Connect changeRequested to the appropriate FolderDialog, normalize selectedFolder with QmlUtils.url_to_path, and keep directory persistence in the relevant backend/settings API. LocationSelector displays the location; it does not open the picker itself.

Centralize closing and cancellation through App.ClosingHandler.handler(...) for active-operation close prompts, including application exit. A cancellation request is not completion: wait for the worker to finish before closing or destroying its window. Extend the existing ClosingHandler when an operation needs a new close-handling category instead of adding a separate prompt flow.

When transforming from C++ to QML, keep the comments, fixme or todo comments in the code, if you do not know how to transform some code add a fixme  and a explanatory comment about what it is supposed to do

In addition to qmetaobject-rs we have a custom macro for QtThreading to safely manage threading and communication between the Qt event loop.


You can see in some Rust structs we have #[derive(QtThreading)], that generates a thread-safe wrapper around the struct whenever you need to fire events you need to get the instance

```rust
        let q_thread = self.qt_thread();

        //then in an async or sync context you may do
        q_thread.queue(|t| {
            t.state = state;
            t.state_changed();
        });
```

If it's a variable we need the $var_changed() signal to be emitted so something like so needed while creating structs 

```rust
    state: qt_property!(QVariantMap; NOTIFY  state_changed),
    state_changed: qt_signal!(),
```


complete example

```rust
    use crate::{RUNTIME, qt_threading::QtThreading, qvariantmap_insert};
    use macros::QtThreading;
    use qmetaobject::prelude::*;

    #[derive(QObject, Default, QtThreading)]
    pub struct Apps {
        base: qt_base_class!(trait QObject),
        state: qt_property!(QVariantMap; NOTIFY  state_changed),
        state_changed: qt_signal!(),
        ipa_tool: Option<Arc<IpaTool>>,
        init: qt_method!(fn(&mut self)),
        sign_in: qt_method!(fn(&mut self, email: QString, password: QString)),
        search: qt_method!(fn(&mut self, term: QString)),
        search_ready: qt_signal!(search_term : QString, success: bool, res: QString),
    }

```

We have some globals defined in qml context which are accessible in any qml file (you can see more in main.rs)


```rust
    let mut engine = QmlEngine::new();

    let core_obj = QObjectBox::new(core::Core::default());
    engine.set_object_property("core".into(), core_obj.pinned());

    let obj = QObjectBox::new(image_loader::ImageLoader::default());
    engine.set_object_property("imageLoader".into(), obj.pinned());

    let apps_impl = QObjectBox::new(apps::Apps::new_with_state());
    engine.set_object_property("apps".into(), apps_impl.pinned());

    let provider_ref_cell = QObjectBox::new(image_provider::ImageProvider::default(obj));
    engine.add_image_provider("thumb", provider_ref_cell);

    let airplay = QObjectBox::new(airplay::Airplay::default());
    engine.set_object_property("AirplayImp".into(), airplay.pinned());
```

If the thing can or should be singleton you need to follow a similar approch or ask me whether it should be a singleton or not.

Use camelCase for signal names in Rust and QML, only for signals not for methods or properties

Use url_to_path from QmlUtils for the selected path to normalize the path, for example when you get a file path from a FileDialog you need to normalize it before using it

Sometimes you will asked to tranform some QWidgets C++ code to QML in this case if you see a varible called loadingWidget that's the equivalent of
StateView in qml which you can import like so (assuming you are in a qml file in src/ui/ , adjust the path accordingly if you are in a different folder)

```qml
import "./base"
```


An example usage for StateView would be like so

```qml

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts 
import "./base"

Dialog {
    id: dlg
    modal: true
    focus: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    anchors.centerIn: parent

    width: 560
    height: 560

    property int currentIndex: 0
    property bool loading: true

    Timer {
        id: loadTimer
        interval: 300
        repeat: false
        onTriggered: stateView.viewState = StateView.State.Content
    }

    function updateNav() {
        prevBtn.enabled = dlg.currentIndex > 0
        nextBtn.enabled = dlg.currentIndex < (stack.count - 1)
    }

    onCurrentIndexChanged: updateNav()
    Component.onCompleted: {
        updateNav()
        loadTimer.start()
    }

    background: Rectangle {
        radius: 10
        color: palette.window
        border.color: Qt.rgba(0, 0, 0, 0.12)
        border.width: 1
    }

    contentItem: StateView {
        id: stateView
        anchors.fill: parent
        viewState: StateView.State.Loading 
        contentItem : ColumnLayout {
            anchors.fill: parent
            anchors.margins: 16
            spacing: 12

            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true

                StackLayout {
                    id: stack
                    anchors.fill: parent
                    currentIndex: dlg.currentIndex

                    // Page 1
                    Item {
                        ColumnLayout {
                            anchors.fill: parent
                            spacing: 10

                            Item { Layout.fillHeight: true }

                            Text {
                                Layout.fillWidth: true
                                text: "Connect your device"
                                horizontalAlignment: Text.AlignHCenter
                                wrapMode: Text.WordWrap
                                font.pixelSize: 16
                                font.bold: true
                                color: palette.text
                            }

                            Image {
                                Layout.alignment: Qt.AlignHCenter
                                source: "qrc:/resources/connect.png"
                                fillMode: Image.PreserveAspectFit
                                smooth: true
                                mipmap: true
                                Layout.preferredWidth: 200
                                Layout.preferredHeight: 200
                            }

                            Item { Layout.fillHeight: true }
                        }
                    }

                    // Page 2
                    Item {
                        ColumnLayout {
                            anchors.fill: parent
                            spacing: 10

                            Item { Layout.fillHeight: true }

                            Text {
                                Layout.fillWidth: true
                                text: "Accept the pairing dialog"
                                horizontalAlignment: Text.AlignHCenter
                                wrapMode: Text.WordWrap
                                font.pixelSize: 16
                                font.bold: true
                                color: palette.text
                            }

                            Image {
                                Layout.alignment: Qt.AlignHCenter
                                source: "qrc:/resources/trust.png"
                                fillMode: Image.PreserveAspectFit
                                smooth: true
                                mipmap: true
                                Layout.preferredWidth: 200
                                Layout.preferredHeight: 200
                            }

                            Item { Layout.fillHeight: true }
                        }
                    }

                    // Page 3
                    Item {
                        ColumnLayout {
                            anchors.fill: parent
                            spacing: 10

                            Item { Layout.fillHeight: true }

                            Text {
                                Layout.fillWidth: true
                                text: {
                                    if (Qt.platform.os === "windows")
                                        return qsTr("You can now unplug the device. iDescriptor will connect to it automatically (requires iOS 15 or later and the Bonjour service).")
                                    if (Qt.platform.os === "linux")
                                        return qsTr("You can now unplug the device. iDescriptor will connect to it automatically (requires iOS 15 or later and the Avahi daemon).")
                                    return qsTr("You can now unplug the device. iDescriptor will connect to it automatically (requires iOS 15 or later).")
                                }
                                horizontalAlignment: Text.AlignHCenter
                                wrapMode: Text.WordWrap
                                font.pixelSize: 16
                                font.bold: true
                                color: palette.text
                            }

                            Image {
                                Layout.alignment: Qt.AlignHCenter
                                source: "qrc:/resources/ios-version.png"
                                fillMode: Image.PreserveAspectFit
                                smooth: true
                                mipmap: true
                                Layout.preferredWidth: 200
                                Layout.preferredHeight: 200
                            }

                            Item { Layout.fillHeight: true }
                        }
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 10

                Item { Layout.fillWidth: true }

                Button {
                    id: prevBtn
                    icon.source: "qrc:/resources/icons/material-symbols_arrow-left-alt.svg"
                    Layout.preferredWidth: 48
                    onClicked: {
                        if (dlg.currentIndex > 0) dlg.currentIndex -= 1
                    }
                }

                Button {
                    id: nextBtn
                    icon.source: "qrc:/resources/icons/material-symbols_arrow-right-alt.svg"
                    Layout.preferredWidth: 48
                    onClicked: {
                        if (dlg.currentIndex < stack.count - 1) dlg.currentIndex += 1
                    }
                }

                Item { Layout.fillWidth: true }
            }
        }
    }
}

```

You should only implement StateView if asked, or if you are transforming a QWidget (C++ code) that has a loadingWidget variable

If you see QStackedWidget in C++ code and it's just for loading states then use StateView in QML 


You can use `qvariantmap_insert!` macro to insert values into a QVariantMap in Rust like so

```rust
    use crate::qvariantmap_insert;
    let mut state = QVariantMap::default();
    qvariantmap_insert!(state, "init", true);
    qvariantmap_insert!(state, "error", QString::default());
    qvariantmap_insert!(state, "email", QString::from(acc.email));
``` 

Use `RUNTIME.spawn(async move { ... })` to spawn async tasks in Rust when needed


For example

```rust
    use crate::{RUNTIME, qt_threading::QtThreading, qvariantmap_insert};
    use ipatool::Account;
    use ipatool::IpaTool;
    use macros::QtThreading;
    use qmetaobject::prelude::*;
    use qttypes::{QStringList, QVariantMap};
    use std::pin::Pin;
    use std::sync::Arc;

    #[derive(QObject, Default, QtThreading)]
    pub struct Apps {
        base: qt_base_class!(trait QObject),
        state: qt_property!(QVariantMap; NOTIFY  state_changed),
        state_changed: qt_signal!(),
        ipa_tool: Option<Arc<IpaTool>>,
        init: qt_method!(fn(&mut self)),
        sign_in: qt_method!(fn(&mut self, email: QString, password: QString)),
        search: qt_method!(fn(&mut self, term: QString)),
        search_ready: qt_signal!(search_term : QString, success: bool, res: QString),
    }

    impl Apps {
        pub fn new_with_state() -> Self {
            let mut state = QVariantMap::default();
            qvariantmap_insert!(state, "init", false);
            qvariantmap_insert!(state, "error", QString::default());
            qvariantmap_insert!(state, "email", QString::default());

            let mut def = Self::default();
            def.state = state;
            def
        }

        fn init(&mut self) {
            let q_thread = self.qt_thread();
            RUNTIME.spawn(async move {
                let res: anyhow::Result<(Option<ipatool::Account>, IpaTool)> = async {
                    let tool = IpaTool::new_default().await?;
                    Ok((tool.account_info().await?, tool))
                }
                .await;

                match res {
                    Ok((maybe_acc, tool)) => {
                        let acc = maybe_acc.unwrap_or_default();
                        println!("email :{}", acc.email);

                        let mut state = QVariantMap::default();
                        qvariantmap_insert!(state, "init", true);
                        qvariantmap_insert!(state, "error", QString::default());
                        qvariantmap_insert!(state, "email", QString::from(acc.email));

                        q_thread.queue(|t| {
                            t.state = state;
                            t.ipa_tool = Some(Arc::new(tool));
                            t.state_changed();
                        })
                    }
                    Err(err) => {
                        let mut state = QVariantMap::default();
                        qvariantmap_insert!(state, "init", true);
                        qvariantmap_insert!(state, "error", QString::from(format!("{}", err)));
                        qvariantmap_insert!(state, "email", QString::default());

                        q_thread.queue(|t| {
                            t.state = state;
                            t.state_changed();
                        });
                    }
                }
            });
        }
    }
```

Always use `qsTr` for internationalization in QML files like so, do not hardcode strings without it (QML files only)

```qml
Text {
    text: qsTr("Hello World")
}
```

When transforming from C++ to QML, make sure to not delete any fixme, todo or any explanatory comments


Use IconLoader in QML if you want to render icons for bundleIds or icons that need to be fetched asynchronously

Example usage of IconLoader

```qml
    IconLoader {
        iconSource: root.iconSource
    }
```


If you need to know device context you can use 


```qml
    import "." as App
    //then for example

    StackLayout {
        anchors.fill: parent
        currentIndex:  App.DeviceContext.showWelcomePage ? 1 : 0
    }
```



DeviceContext is a singleton, read the code in src/ui/DeviceContext.qml if you need to know more


Do not create extra vars like root.deviceList when you can use DeviceContext.devices directly in QML

DeviceContext might look something like this in QML

```qml
    QtObject {
        id: root
        property ListModel devices: ListModel {}
        property string currentDeviceUdid : ""
        // default to info section
        property int currentSection : 0 
        property int currentTab: 0
        property bool showWelcomePage : true
    }
```
Read the file src/ui/DeviceContext.qml for upto date information about DeviceContext, it might change in the future

All singletons can be found in file src/ui/qmldir, yes src/ui/qmldir is a file not a folder

Use ToolWindow for windows that are used in toolbox tab, 

Tools will always receive

```qml
   required property string udid
   required property var device
```


Tools will always auto-close

```qml
    Component.onCompleted : {
        App.DeviceContext.device_removed.connect((udid) => {
            if (root.udid === udid) {root.close()}
        })
    }
```


Do not use versions in imports in QML

```qml
    import QtQuick
    import QtQuick.Controls
    import QtQuick.Layouts
```


Use reqwest for network requests in Rust


Use tokio runtime for async tasks in Rust, 


you have to use RUNTIME.spawn(async move { ... }) to spawn async tasks

do not implement shared_instance methods in Rust, whether when transforming from C++ to Rust or when creating new structs


Use anyhow for error types


Use log create on Rust code whenever possbile for debugging purposes
```rs
    use log::{debug, info, warn, error};

    error!("Something went wrong: {}", some_value);
    warn!("This might be a problem");
    info!("Server started on port {}", port);
    debug!("Processing item: {:?}", item); 
```

Use best practices


Do not check for null on things that are already defined and set from main.rs


For example qml_utils is already set in main.rs and you can use it directly in any qml file
```rust
let qml_utils = QObjectBox::new(qml_utils::QmlUtils::new(engine_ptr));
engine.set_object_property("QmlUtils".into(), qml_utils.pinned());

```

// Avoid
```qml
//Do not do 

    property var qmlUtils: typeof QmlUtils == "undefined" ? null : QmlUtils

//or
    Button {
        onClicked: {
            if (typeof QmlUtils !== "undefined") {
                QmlUtils.someMethod()
            }
        }
    }
```
