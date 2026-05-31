/*
 * iDescriptor: A free and open-source idevice management tool.
 *
 * Copyright (C) 2025 Uncore <https://github.com/uncor3>
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program. If not, see <https://www.gnu.org/licenses/>.
 */

#include "afcexplorerwidget.h"
#include "iDescriptor-ui.h"
#include "iDescriptor.h"
#include "mediapreviewdialog.h"
#include "settingsmanager.h"
#include <QDebug>
#include <QDesktopServices>
#include <QFileDialog>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QIcon>
#include <QInputDialog>
#include <QMenu>
#include <QMessageBox>
#include <QPushButton>
#include <QRegularExpression>
#include <QScrollBar>
#include <QSignalBlocker>
#include <QSplitter>
#include <QStackedWidget>
#include <QStyle>
#include <QTemporaryDir>
#include <QTreeWidget>
#include <QVariant>

AfcExplorerWidget::AfcExplorerWidget(
    const std::shared_ptr<iDescriptorDevice> device, bool favEnabled,
    std::optional<std::shared_ptr<CXX::HauseArrest>> hause_arrest, bool useAfc2,
    QString root, QWidget *parent)
    : QWidget(parent), m_device(device), m_favEnabled(favEnabled),
      m_hauseArrest(hause_arrest), m_errorMessage("Failed to load directory"),
      m_root(root), m_useAfc2(useAfc2)
{

    QVBoxLayout *rootLayout = new QVBoxLayout(this);
    rootLayout->setContentsMargins(0, 0, 0, 0);

    // Setup file explorer
    setupFileExplorer();

    // Main layout
    QWidget *contentContainer = new QWidget();
    QHBoxLayout *contentLayout = new QHBoxLayout(contentContainer);
    contentLayout->setContentsMargins(0, 0, 0, 0);

    contentLayout->addWidget(m_explorer);

    // Initialize
    m_history.push(m_root);
    m_currentHistoryIndex = 0;
    m_forwardHistory.clear();

    m_loadingWidget = new ZLoadingWidget(true, this);
    rootLayout->addWidget(m_loadingWidget);
    m_loadingWidget->setupContentWidget(contentContainer);

    connect(m_loadingWidget, &ZLoadingWidget::retryClicked, this, [this]() {
        m_loadingWidget->showLoading();
        QTimer::singleShot(100, this, [this]() { loadPath(m_history.top()); });
    });

    if (m_useAfc2) {
        bool is_available = m_device->afc2_backend->is_available();
        if (!is_available) {
            qDebug()
                << "[AfcExplorerWidget] AFC2 is not available on this device.";
            m_loadingWidget->showError("AFC2 is not available on this device.");
            return;
        }
    }

    if (m_useAfc2) {
        connect(m_device->afc2_backend,
                &CXX::Afc2Backend::check_is_dir_and_list_finished, this,
                &AfcExplorerWidget::onLoadPathFinished);
    } else if (m_hauseArrest.has_value() && m_hauseArrest.value() != nullptr) {
        connect(m_hauseArrest.value().get(),
                &CXX::HauseArrest::check_is_dir_and_list_finished, this,
                &AfcExplorerWidget::onLoadPathFinished);
    } else {
        connect(m_device->afc_backend,
                &CXX::AfcBackend::check_is_dir_and_list_finished, this,
                &AfcExplorerWidget::onLoadPathFinished);
    }

    loadPath(m_root);

    setupContextMenu();
}

void AfcExplorerWidget::goBack()
{
    if (m_history.size() > 1) {
        // Move current path to forward history
        QString currentPath = m_history.pop();
        m_forwardHistory.push(currentPath);

        QString prevPath = m_history.top();
        loadPath(prevPath);
    }
}

void AfcExplorerWidget::goForward()
{
    if (!m_forwardHistory.isEmpty()) {
        QString forwardPath = m_forwardHistory.pop();
        m_history.push(forwardPath);
        loadPath(forwardPath);
    }
}

void AfcExplorerWidget::onItemDoubleClicked(QListWidgetItem *item)
{
    QVariant data = item->data(Qt::UserRole);
    bool isDir = data.toBool();
    QString name = item->text();

    // Use breadcrumb to get current path
    QString currPath = "/";
    if (!m_history.isEmpty())
        currPath = m_history.top();

    if (!currPath.endsWith("/"))
        currPath += "/";
    QString nextPath = currPath == "/" ? "/" + name : currPath + name;

    if (isDir) {
        // Clear forward history when navigating to a new directory
        m_forwardHistory.clear();
        m_history.push(nextPath);
        loadPath(nextPath);
    } else {
        const bool isPreviewable = iDescriptor::Utils::isPreviewableFile(name);
        if (isPreviewable) {
            auto *previewDialog = new MediaPreviewDialog(
                m_device, nextPath, m_hauseArrest, m_useAfc2, this);
            previewDialog->setAttribute(Qt::WA_DeleteOnClose);
            previewDialog->show();
        } else {
            openWithDesktopService(item);
        }
    }
}

void AfcExplorerWidget::openWithDesktopService(QListWidgetItem *item)
{
    QTemporaryDir *tempDir = new QTemporaryDir();
    if (!tempDir->isValid()) {
        QMessageBox::critical(this, "Error",
                              "Could not create a temporary directory.");
        delete tempDir;
        return;
    }

    exportAndOpenSelectedFile(item, tempDir->path());
}

void AfcExplorerWidget::onAddressBarReturnPressed()
{
    QString path = m_addressBar->text().trimmed();
    if (path.isEmpty()) {
        path = "/";
    }

    // Normalize the path
    if (!path.startsWith("/")) {
        path = "/" + path;
    }

    // Remove duplicate slashes
    path = path.replace(QRegularExpression("/+"), "/");

    // Clear forward history when navigating to a new path
    m_forwardHistory.clear();

    // Update history and load the path
    m_history.push(path);
    loadPath(path);
}

void AfcExplorerWidget::updateNavigationButtons()
{
    // Update button states based on history
    if (m_backButton) {
        m_backButton->setEnabled(m_history.size() > 1);
    }
    if (m_forwardButton) {
        m_forwardButton->setEnabled(!m_forwardHistory.isEmpty());
    }
    if (m_upButton) {
        bool canGoUp = !m_history.isEmpty() && m_history.top() != "/";
        m_upButton->setEnabled(canGoUp);
    }
}

void AfcExplorerWidget::updateAddressBar(const QString &path)
{
    // Update the address bar with the current path
    m_addressBar->setText(path);
}

void AfcExplorerWidget::loadPath(const QString &path)
{
    m_loadingWidget->showLoading();

    if (m_useAfc2) {
        bool is_available = m_device->afc2_backend->is_available();
        if (!is_available) {
            qDebug()
                << "[AfcExplorerWidget] AFC2 is not available on this device.";
            m_loadingWidget->showError("AFC2 is not available on this device.");
            return;
        }
    }

    updateAddressBar(path);
    updateNavigationButtons();

    // FIXME: we need a better approach to this
    // similar code is repeated in some places
    /* use the correct afc client */
    if (m_useAfc2) {
        m_device->afc2_backend->check_is_dir_and_list(path);
    } else if (m_hauseArrest.has_value() && m_hauseArrest.value() != nullptr) {
        m_hauseArrest.value()->check_is_dir_and_list(path);
    } else {
        m_device->afc_backend->check_is_dir_and_list(path);
    }
}

void AfcExplorerWidget::onLoadPathFinished(bool success,
                                           const QMap<QString, QVariant> &tree)
{
    m_fileList->clear();
    showFileListState();

    for (auto it = tree.constBegin(); it != tree.constEnd(); ++it) {
        bool is_dir = it.value().toBool();

        QListWidgetItem *item = new QListWidgetItem(it.key());
        item->setData(Qt::UserRole, is_dir);
        if (is_dir) {
            QIcon folderIcon = QIcon::fromTheme("folder");
            if (folderIcon.isNull()) {
                item->setIcon(
                    QIcon(":/resources/icons/MaterialSymbolsFolder.png"));
            } else {
                item->setIcon(folderIcon);
            }
        } else {
            QIcon fileIcon = QIcon::fromTheme("text-x-generic");
            if (fileIcon.isNull()) {
                item->setIcon(
                    QIcon(":/resources/icons/IcBaselineInsertDriveFile.png"));
            } else {
                item->setIcon(fileIcon);
            }
        }
        m_fileList->addItem(item);
    }

    m_loadingWidget->stop();
}

void AfcExplorerWidget::setupContextMenu()
{
    m_fileList->setContextMenuPolicy(Qt::CustomContextMenu);
    connect(m_fileList, &QListWidget::customContextMenuRequested, this,
            &AfcExplorerWidget::onFileListContextMenu);
}

void AfcExplorerWidget::onFileListContextMenu(const QPoint &pos)
{
    QListWidgetItem *item = m_fileList->itemAt(pos);
    if (!item)
        return;

    bool isDir = item->data(Qt::UserRole).toBool();
    if (isDir)
        return; // TODO: Implement directory export later - Only export files
                // for now

    QMenu menu;
    QAction *exportAction = menu.addAction("Export");
    QAction *openAction = menu.addAction("Open");
    QAction *openNativeAction = menu.addAction("Open Externally");
    QAction *selectedAction =
        menu.exec(m_fileList->viewport()->mapToGlobal(pos));
    if (selectedAction == exportAction) {
        QList<QListWidgetItem *> selectedItems = m_fileList->selectedItems();
        QList<QListWidgetItem *> filesToExport;
        if (selectedItems.isEmpty())
            filesToExport.append(item); // fallback: just the clicked one
        else {
            for (QListWidgetItem *selItem : selectedItems) {
                if (!selItem->data(Qt::UserRole).toBool())
                    filesToExport.append(selItem);
            }
        }
        if (filesToExport.isEmpty())
            return;

        handleExport(filesToExport);

    } else if (selectedAction == openAction) {
        onItemDoubleClicked(item);
    } else if (selectedAction == openNativeAction) {
        openWithDesktopService(item);
    }
}

void AfcExplorerWidget::onExportClicked()
{
    QList<QListWidgetItem *> selectedItems = m_fileList->selectedItems();
    if (selectedItems.isEmpty())
        return;

    // Only files (not directories) - TODO: Implement directory export later
    QList<QListWidgetItem *> filesToExport;
    for (QListWidgetItem *item : selectedItems) {
        if (!item->data(Qt::UserRole).toBool())
            filesToExport.append(item);
    }
    if (filesToExport.isEmpty())
        return;

    handleExport(filesToExport);
}

void AfcExplorerWidget::handleExport(QList<QListWidgetItem *> filesToExport)
{
    QString dir =
        QFileDialog::getExistingDirectory(this, "Select Export Directory", QString(),
                                         QFileDialog::ShowDirsOnly | QFileDialog::DontResolveSymlinks | QFileDialog::DontUseNativeDialog);
    if (dir.isEmpty())
        return;

    QList<QString> exportItems;
    QString currPath = "/";
    if (!m_history.isEmpty())
        currPath = m_history.top();
    if (!currPath.endsWith("/"))
        currPath += "/";

    for (QListWidgetItem *selItem : filesToExport) {
        QString fileName = selItem->text();
        QString devicePath =
            currPath == "/" ? "/" + fileName : currPath + fileName;
        exportItems.append(devicePath);
    }

    if (m_useAfc2) {
        IOManagerClient::sharedInstance()->startExport(
            m_device, exportItems, dir, "Exporting from File Explorer <AFC2>",
            true);
    } else if (m_hauseArrest.has_value() && m_hauseArrest.value() != nullptr) {
        IOManagerClient::sharedInstance()->startExport(
            m_device, exportItems, dir,
            "Exporting from File Explorer (App Container)",
            m_hauseArrest.value()->get_bundle_id());
    } else {
        IOManagerClient::sharedInstance()->startExport(
            m_device, exportItems, dir, "Exporting from File Explorer");
    }
}

void AfcExplorerWidget::exportAndOpenSelectedFile(QListWidgetItem *item,
                                                  const QString &directory)
{
    if (!QDir(directory).exists()) {
        QMessageBox::critical(this, "Error",
                              "Could not access the temporary directory.");
        return;
    }

    QList<QString> exportItems;
    QString currPath = "/";
    if (!m_history.isEmpty())
        currPath = m_history.top();
    if (!currPath.endsWith("/"))
        currPath += "/";

    QString fileName = item->text();
    QString devicePath = currPath == "/" ? "/" + fileName : currPath + fileName;
    exportItems.append(devicePath);

    QString localPath = QDir(directory).filePath(fileName);

    std::function<void()> onExportFinished = [localPath]() {
        QDesktopServices::openUrl(QUrl::fromLocalFile(localPath));
    };

    if (m_useAfc2) {
        IOManagerClient::sharedInstance()->startExport(
            m_device, exportItems, directory,
            "Exporting from File Explorer <AFC2>", true, onExportFinished);
    } else if (m_hauseArrest.has_value() && m_hauseArrest.value() != nullptr) {
        IOManagerClient::sharedInstance()->startExport(
            m_device, exportItems, directory,
            "Exporting from File Explorer (App Container)",
            m_hauseArrest.value()->get_bundle_id(), onExportFinished);
    } else {
        IOManagerClient::sharedInstance()->startExport(
            m_device, exportItems, directory, "Exporting from File Explorer",
            onExportFinished);
    }
}

void AfcExplorerWidget::onImportClicked()
{
    QStringList fileNames = QFileDialog::getOpenFileNames(this, "Import Files", QString(),
                                                         "All Files (*)", nullptr, QFileDialog::DontUseNativeDialog);
    if (fileNames.isEmpty())
        return;

    QString currPath = "/";
    if (!m_history.isEmpty())
        currPath = m_history.top();
    if (!currPath.endsWith("/"))
        currPath += "/";

    QPointer safeThis(this);
    std::function<void()> onImportFinished = [this, currPath, safeThis]() {
        if (!safeThis || safeThis.isNull())
            return;
        QTimer::singleShot(100, this, [this]() {
            if (!m_history.isEmpty())
                loadPath(m_history.top());
        });
    };
    if (m_useAfc2) {
        IOManagerClient::sharedInstance()->startImport(
            m_device, fileNames, currPath, "Importing <AFC2>", true,
            onImportFinished);
    } else if (m_hauseArrest.has_value() && m_hauseArrest.value() != nullptr) {
        IOManagerClient::sharedInstance()->startImport(
            m_device, fileNames, currPath, "Importing to App Container",
            m_hauseArrest.value()->get_bundle_id(), onImportFinished);
    } else {
        IOManagerClient::sharedInstance()->startImport(
            m_device, fileNames, currPath, "Importing", onImportFinished);
    }
}

void AfcExplorerWidget::setupFileExplorer()
{
    m_explorer = new QWidget();
    QVBoxLayout *explorerLayout = new QVBoxLayout(m_explorer);
    explorerLayout->setContentsMargins(0, 0, 0, 0);
    m_explorer->setStyleSheet("border : none;");

    // Export/Import buttons layout
    m_exportBtn =
        new ZIconWidget(QIcon(":/resources/icons/PhExport.png"), "Export");
    m_importBtn = new ZIconWidget(
        QIcon(":/resources/icons/LetsIconsImport.png"), "Import");
    if (m_favEnabled) {
        m_addToFavoritesBtn = new ZIconWidget(
            QIcon(":/resources/icons/MaterialSymbolsFavorite.png"),
            "Add to Favorites");
    }

    // Navigation layout (Address Bar with embedded icons)
    m_navWidget = new QWidget();
    m_navWidget->setObjectName("navWidget");
    m_navWidget->setFocusPolicy(Qt::StrongFocus); // Make it focusable

    m_navWidget->setMaximumWidth(500);
    m_navWidget->setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Fixed);

    QHBoxLayout *navContainerLayout = new QHBoxLayout();
    navContainerLayout->addStretch();
    navContainerLayout->addWidget(m_navWidget);
    navContainerLayout->addStretch();

    QHBoxLayout *navLayout = new QHBoxLayout(m_navWidget);
    navLayout->setContentsMargins(0, 0, 0, 0);
    navLayout->setSpacing(0);

    QWidget *explorerLeftSideNavButtons = new QWidget();
    QHBoxLayout *leftNavLayout = new QHBoxLayout(explorerLeftSideNavButtons);

    leftNavLayout->setContentsMargins(0, 0, 0, 0);
    leftNavLayout->setSpacing(1);
    m_backButton = new ZIconWidget(
        QIcon(":/resources/icons/MaterialSymbolsArrowLeftAlt.png"), "Go Back");
    m_backButton->setEnabled(false);

    m_forwardButton = new ZIconWidget(
        QIcon(":/resources/icons/MaterialSymbolsArrowRightAlt.png"),
        "Go Forward");
    m_forwardButton->setEnabled(false);

    m_homeButton = new ZIconWidget(
        QIcon(":/resources/icons/MaterialSymbolsLightHome.png"), "Go Home");

    m_upButton = new ZIconWidget(
        QIcon(":/resources/icons/MaterialSymbolsArrowUpwardAltRounded.png"),
        "Go Up");
    m_upButton->setEnabled(false);

    m_enterButton = new ZIconWidget(
        QIcon(":/resources/icons/MaterialSymbolsLightKeyboardReturn.png"),
        "Navigate to path");

    m_deleteButton = new ZIconWidget(
        QIcon(":/resources/icons/MaterialSymbolsDelete.png"), "Delete");

    m_addressBar = new QLineEdit();
    m_addressBar->setPlaceholderText("Enter path...");
    m_addressBar->setText("/");

    // Add widgets to navigation layout
    leftNavLayout->addWidget(m_backButton);
    leftNavLayout->addWidget(m_forwardButton);
    leftNavLayout->addWidget(m_homeButton);
    leftNavLayout->addWidget(m_upButton);
    navLayout->addWidget(explorerLeftSideNavButtons);
    navLayout->addWidget(m_addressBar);
    navLayout->addWidget(m_importBtn);
    navLayout->addWidget(m_exportBtn);
    navLayout->addWidget(m_deleteButton);
    if (m_favEnabled)
        navLayout->addWidget(m_addToFavoritesBtn);

    navLayout->addWidget(m_enterButton);

    // Add the container layout (which centers navWidget) to the main layout
    explorerLayout->addLayout(navContainerLayout);

    // Create stacked widget for content (file list or error state)
    m_contentStack = new QStackedWidget();

    // Create file list widget
    m_fileListWidget = new QWidget();
    QVBoxLayout *fileListLayout = new QVBoxLayout(m_fileListWidget);
    fileListLayout->setContentsMargins(0, 0, 0, 0);

    // File list
    m_fileList = new QListWidget();
    m_fileList->setSelectionMode(QAbstractItemView::ExtendedSelection);
#ifdef WIN32
    m_fileList->setStyleSheet(R"(
    QScrollBar:vertical {
        border: 6px solid rgba(0, 0, 0, 0);
        margin: 14px 0px 14px 0px;
        width: 16px;
        background-color: transparent;
    }
    QScrollBar::handle:vertical {
        background-color: rgba(0, 0, 0, 110);
        border-radius: 2px;
        min-height: 25px;
    }
    )");
#endif
    fileListLayout->addWidget(m_fileList);

    // Create error widget
    m_errorWidget = new QWidget();
    QVBoxLayout *errorLayout = new QVBoxLayout(m_errorWidget);
    errorLayout->setContentsMargins(20, 20, 20, 20);

    errorLayout->addStretch();

    m_errorLabel = new QLabel(m_errorMessage);
    m_errorLabel->setAlignment(Qt::AlignCenter);
    m_errorLabel->setWordWrap(true);
    errorLayout->addWidget(m_errorLabel);

    m_retryButton = new QPushButton("Try Again");
    m_retryButton->setMaximumWidth(120);
    QHBoxLayout *buttonLayout = new QHBoxLayout();
    buttonLayout->addStretch();
    buttonLayout->addWidget(m_retryButton);
    buttonLayout->addStretch();
    errorLayout->addLayout(buttonLayout);

    errorLayout->addStretch();

    // Add both widgets to the stacked widget
    m_contentStack->addWidget(m_fileListWidget);
    m_contentStack->addWidget(m_errorWidget);

    // Start with file list view
    m_contentStack->setCurrentWidget(m_fileListWidget);

    explorerLayout->addWidget(m_contentStack);

    // Connect buttons and actions
    connect(m_backButton, &ZIconWidget::clicked, this,
            &AfcExplorerWidget::goBack);
    connect(m_forwardButton, &ZIconWidget::clicked, this,
            &AfcExplorerWidget::goForward);
    connect(m_homeButton, &ZIconWidget::clicked, this,
            &AfcExplorerWidget::goHome);
    connect(m_upButton, &ZIconWidget::clicked, this, &AfcExplorerWidget::goUp);
    connect(m_enterButton, &ZIconWidget::clicked, this,
            &AfcExplorerWidget::onAddressBarReturnPressed);
    connect(m_addressBar, &QLineEdit::returnPressed, this,
            &AfcExplorerWidget::onAddressBarReturnPressed);
    connect(m_fileList, &QListWidget::itemDoubleClicked, this,
            &AfcExplorerWidget::onItemDoubleClicked);
    connect(m_exportBtn, &ZIconWidget::clicked, this,
            &AfcExplorerWidget::onExportClicked);
    connect(m_importBtn, &ZIconWidget::clicked, this,
            &AfcExplorerWidget::onImportClicked);
    connect(m_retryButton, &QPushButton::clicked, this,
            &AfcExplorerWidget::onRetryClicked);
    connect(m_deleteButton, &ZIconWidget::clicked, this,
            &AfcExplorerWidget::onDeleteClicked);
    connect(m_fileList->selectionModel(),
            &QItemSelectionModel::selectionChanged, this,
            &AfcExplorerWidget::updateButtonStates);

    if (m_favEnabled) {
        connect(m_addToFavoritesBtn, &ZIconWidget::clicked, this,
                &AfcExplorerWidget::onAddToFavoritesClicked);
    }

    updateNavigationButtons();
    updateButtonStates();
#ifndef WIN32
    updateNavStyles();
#endif
}

void AfcExplorerWidget::onAddToFavoritesClicked()
{
    QString currentPath = "/";
    if (!m_history.isEmpty())
        currentPath = m_history.top();

    bool ok;
    QString alias = QInputDialog::getText(
        this, "Add to Favorites",
        "Enter alias for this location:", QLineEdit::Normal, "Alias here", &ok);
    if (ok && !alias.isEmpty()) {
        emit favoritePlaceAdded(alias, currentPath);
    } else if (ok && alias.isEmpty()) {
        QMessageBox::warning(nullptr, "Invalid Input", "Alias was empty.");
        qWarning() << "Cannot save favorite place with empty alias";
    } else if (!ok) {
        qWarning() << "Failed to get alias for favorite place";
    }
}

#ifndef WIN32
void AfcExplorerWidget::updateNavStyles()
{
    if (!m_navWidget || !m_addressBar)
        return;
    bool isDark = isDarkMode();
    QColor lightColor = qApp->palette().color(QPalette::Light);
    QColor darkColor = qApp->palette().color(QPalette::Dark);
    QColor bgColor = isDark ? lightColor : darkColor;
    QColor borderColor = qApp->palette().color(QPalette::Mid);
    QColor accentColor = qApp->palette().color(QPalette::Highlight);

    QString navStyles = QString("QWidget#navWidget {"
                                "    background-color: %1;"
                                "    border: 1px solid %2;"
                                "    border-radius: 10px;"
                                "}"
                                "QWidget#navWidget {"
                                "    outline: 1px solid %3;"
                                "    outline-offset: 1px;"
                                "}")
                            .arg(bgColor.name())
                            .arg(bgColor.lighter().name())
                            .arg(accentColor.name());

    if (m_navWidget->styleSheet() != navStyles)
        m_navWidget->setStyleSheet(navStyles);

    // Update address bar styles to complement the nav widget
    QString addressBarStyles =
        QString("QLineEdit { background-color: %1; border-radius: 10px; "
                "border: 1px solid %2; padding: 2px 4px; color: %3; }"
                "QLineEdit:focus {border: 3px solid %4; }")
            .arg(isDark ? QColor(Qt::white).name() : QColor(Qt::black).name())
            .arg(borderColor.lighter().name())
            .arg(isDark ? QColor(Qt::black).name() : QColor(Qt::white).name())
            .arg(COLOR_ACCENT_BLUE.name());
    if (m_addressBar->styleSheet() != addressBarStyles)
        m_addressBar->setStyleSheet(addressBarStyles);
}
#endif

void AfcExplorerWidget::updateButtonStates()
{
    QList<QListWidgetItem *> selectedItems = m_fileList->selectedItems();

    bool enteriesDoNotContainDirectories = selectedItems.size() > 0;
    for (QListWidgetItem *item : selectedItems) {
        if (item->data(Qt::UserRole).toBool()) { // a directory
            enteriesDoNotContainDirectories = false;
            break;
        }
    }
    // TODO: implement directory export and remove
    m_exportBtn->setEnabled(enteriesDoNotContainDirectories);
    m_deleteButton->setEnabled(enteriesDoNotContainDirectories);
}

void AfcExplorerWidget::setErrorMessage(const QString &message)
{
    m_errorMessage = message;
    if (m_errorLabel) {
        m_errorLabel->setText(m_errorMessage);
    }
}

void AfcExplorerWidget::showErrorState()
{
    if (m_contentStack) {
        m_contentStack->setCurrentWidget(m_errorWidget);
    }
}

void AfcExplorerWidget::showFileListState()
{
    if (m_contentStack) {
        m_contentStack->setCurrentWidget(m_fileListWidget);
    }
}

void AfcExplorerWidget::onRetryClicked()
{
    QString currentPath = "/";
    if (!m_history.isEmpty()) {
        currentPath = m_history.top();
    }
    loadPath(currentPath);
}

void AfcExplorerWidget::navigateToPath(const QString &path)
{
    if (path.isEmpty()) {
        return;
    }

    QString normalizedPath = path;
    if (!normalizedPath.startsWith("/")) {
        normalizedPath = "/" + normalizedPath;
    }
    normalizedPath = normalizedPath.replace(QRegularExpression("/+"), "/");

    m_history.push(normalizedPath);
    loadPath(normalizedPath);
}

void AfcExplorerWidget::goHome()
{
    // Clear forward history when navigating to a new directory
    m_forwardHistory.clear();
    m_history.push(m_root);
    loadPath(m_root);
}

void AfcExplorerWidget::goUp()
{
    if (m_history.isEmpty()) {
        return;
    }

    QString currentPath = m_history.top();

    // Can't go up from the root directory
    if (currentPath == "/") {
        return;
    }

    // Find the parent directory
    int lastSlashIndex = currentPath.lastIndexOf('/');
    QString parentPath =
        (lastSlashIndex > 0) ? currentPath.left(lastSlashIndex) : "/";

    // Going up is a new navigation action, so clear forward history
    m_forwardHistory.clear();

    // Add the new path to history and load it
    m_history.push(parentPath);
    loadPath(parentPath);
}

void AfcExplorerWidget::onDeleteClicked()
{
    QList<QListWidgetItem *> selectedItems = m_fileList->selectedItems();
    if (selectedItems.isEmpty())
        return;

    QString currPath = "/";
    if (!m_history.isEmpty())
        currPath = m_history.top();
    if (!currPath.endsWith("/"))
        currPath += "/";

    QList<QString> pathsToDelete;
    for (QListWidgetItem *item : selectedItems) {
        QString fileName = item->text();
        QString devicePath =
            currPath == "/" ? "/" + fileName : currPath + fileName;
        pathsToDelete.append(devicePath);
    }

    QMessageBox::StandardButton reply = QMessageBox::question(
        this, "Confirm Deletion",
        QString("Are you sure you want to delete the selected %1 item(s)?")
            .arg(pathsToDelete.size()),
        QMessageBox::Yes | QMessageBox::No);

    bool success = false;

    for (const QString &path : pathsToDelete) {
        if (m_useAfc2) {
            success = m_device->afc2_backend->delete_path(path);
        } else if (m_hauseArrest.has_value() &&
                   m_hauseArrest.value() != nullptr) {
            success = m_hauseArrest.value()->delete_path(path);
        } else {
            success = m_device->afc_backend->delete_path(path);
        }
    }

    if (!success) {
        QMessageBox::critical(this, "Error",
                              "Failed to delete one or more items.");
    } else {
        // Refresh the current directory after deletion
        QTimer::singleShot(100, this, [this]() {
            if (!m_history.isEmpty())
                loadPath(m_history.top());
        });
    }
}
