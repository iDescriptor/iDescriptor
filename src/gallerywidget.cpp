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

#include "gallerywidget.h"
#include "iDescriptor-ui.h"
#include "iDescriptor.h"
#include "imageloader.h"
#include "iomanagerclient.h"
#include "mediapreviewdialog.h"
#include "photomodel.h"
#include <QComboBox>
#include <QDebug>
#include <QFileDialog>
#include <QFutureWatcher>
#include <QHBoxLayout>
#include <QItemSelectionModel>
#include <QLabel>
#include <QListView>
#include <QMenu>
#include <QMessageBox>
#include <QPushButton>
#include <QRegularExpression>
#include <QStackedWidget>
#include <QStandardItemModel>
#include <QStandardPaths>
#include <QStringList>
#include <QVBoxLayout>
#include <QtConcurrent/QtConcurrent>

/*
    FIXME: this needs to be refactored once we
    figure out how to query Photos.sqlite
    Check out:
    https://github.com/ScottKjr3347/iOS_Local_PL_Photos.sqlite_Queries
*/
GalleryWidget::GalleryWidget(const std::shared_ptr<iDescriptorDevice> device,
                             QWidget *parent)
    : QWidget{parent}, m_device(device)

{
    m_mainLayout = new QVBoxLayout(this);
    m_mainLayout->setContentsMargins(0, 0, 0, 0);
    m_loadingWidget = new ZLoadingWidget(true, this);
    setupControlsLayout();
    m_mainLayout->addWidget(m_loadingWidget);

    // Setup album selection view
    setupAlbumSelectionView();

    // Setup photo gallery view
    setupPhotoGalleryView();

    // Add stacked widget to main layout
    setLayout(m_mainLayout);

    connect(m_loadingWidget, &ZLoadingWidget::retryClicked, this,
            &GalleryWidget::refresh);

    setControlsEnabled(false); // Disable controls until album is selected
}

void GalleryWidget::refresh()
{
    bool inAlbumSelection =
        (m_loadingWidget->currentWidget() == m_albumSelectionWidget);

    m_loadingWidget->showLoading();
    // refresh the album list
    if (inAlbumSelection) {
        qDebug() << "Refreshing album list...";
        QTimer::singleShot(100, this, &GalleryWidget::reload);
        return;
    }
    if (m_model) {
        qDebug() << "Refreshing current album:" << m_currentAlbumPath;
        m_model->setAlbumPath(m_currentAlbumPath);
    }
}

void GalleryWidget::reload()
{
    m_loaded = false;
    load();
}

/*Load is called when the tab is active*/
void GalleryWidget::load()
{
    if (m_loaded)
        return;

    m_loaded = true;
    connect(
        m_device->afc_backend, &CXX::AfcBackend::album_list_loaded, this,
        [this](QString udid, QList<QString> album_list) {
            onAlbumListLoaded(album_list);
        },
        Qt::SingleShotConnection);
    m_device->afc_backend->load_album_list();
}

void GalleryWidget::setupControlsLayout()
{
    m_controlsLayout = new QHBoxLayout();
    m_controlsLayout->setSpacing(5);
    m_controlsLayout->setContentsMargins(7, 7, 7, 7);

    m_importButton = new QPushButton("Import");

    // Sort order combo box
    QLabel *sortLabel = new QLabel("Sort:");
    QFont sortFont = sortLabel->font();
    sortFont.setWeight(QFont::DemiBold);
    sortLabel->setFont(sortFont);

    m_sortComboBox = new QComboBox();
    m_sortComboBox->addItem("Newest First",
                            static_cast<int>(PhotoModel::NewestFirst));
    m_sortComboBox->addItem("Oldest First",
                            static_cast<int>(PhotoModel::OldestFirst));
    m_sortComboBox->setCurrentIndex(0);   // Default to Newest First
    m_sortComboBox->setMinimumWidth(100); // Ensure text fits
    m_sortComboBox->setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);

    // Filter combo box
    QLabel *filterLabel = new QLabel("Filter:");
    QFont filterFont = filterLabel->font();
    filterFont.setWeight(QFont::DemiBold);
    filterLabel->setFont(filterFont);
    m_filterComboBox = new QComboBox();
    m_filterComboBox->addItem("All Media", static_cast<int>(PhotoModel::All));
    m_filterComboBox->addItem("Images Only",
                              static_cast<int>(PhotoModel::ImagesOnly));
    m_filterComboBox->addItem("Videos Only",
                              static_cast<int>(PhotoModel::VideosOnly));
    m_filterComboBox->setCurrentIndex(
        static_cast<int>(PhotoModel::All)); // Default to All
    m_filterComboBox->setMinimumWidth(90);  // Ensure text fits
    m_filterComboBox->setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);

    // Export buttons
    m_exportSelectedButton = new QPushButton("Export Selected");
    m_exportSelectedButton->setEnabled(false);
    m_exportSelectedButton->setSizePolicy(QSizePolicy::Preferred,
                                          QSizePolicy::Fixed);
    m_exportAllButton = new QPushButton("Export All");
    m_exportAllButton->setEnabled(false);

    // Back button
    m_backButton = new ZIconWidget(
        QIcon(":/resources/icons/MaterialSymbolsArrowLeftAlt.png"),
        "Back to Albums");
    m_backButton->setMaximumWidth(30);
    m_backButton->hide(); // Hidden initially

    // Refresh button
    m_refreshButton = new ZIconWidget(
        QIcon(":/resources/icons/IcOutlineRefresh.png"), "Refresh Album");
    m_refreshButton->setMaximumWidth(30);
    connect(m_refreshButton, &ZIconWidget::clicked, this,
            &GalleryWidget::refresh);

    // Connect signals
    connect(m_sortComboBox, QOverload<int>::of(&QComboBox::currentIndexChanged),
            this, &GalleryWidget::onSortOrderChanged);
    connect(m_filterComboBox,
            QOverload<int>::of(&QComboBox::currentIndexChanged), this,
            &GalleryWidget::onFilterChanged);
    connect(m_exportSelectedButton, &QPushButton::clicked, this,
            &GalleryWidget::onExportSelected);
    connect(m_exportAllButton, &QPushButton::clicked, this,
            &GalleryWidget::onExportAll);
    connect(m_backButton, &ZIconWidget::clicked, this,
            &GalleryWidget::onBackToAlbums);

    connect(m_importButton, &QPushButton::clicked, this,
            &GalleryWidget::handleImport);

    // Add widgets to layout
    m_controlsLayout->addWidget(m_backButton);
    m_controlsLayout->addWidget(m_refreshButton);
    m_controlsLayout->addWidget(m_importButton);
    m_controlsLayout->addWidget(sortLabel);
    m_controlsLayout->addWidget(m_sortComboBox);
    m_controlsLayout->addWidget(filterLabel);
    m_controlsLayout->addWidget(m_filterComboBox);
    m_controlsLayout->addStretch(); // Push export buttons to the right
    m_controlsLayout->addWidget(m_exportSelectedButton);
    m_controlsLayout->addWidget(m_exportAllButton);

    QWidget *controlsWidget = new QWidget();
    controlsWidget->setLayout(m_controlsLayout);
    controlsWidget->setObjectName("controlsWidget");
    controlsWidget->setStyleSheet("QWidget#controlsWidget { "
                                  "  padding: 2px; "
                                  "}");

    m_mainLayout->addWidget(controlsWidget);
}

void GalleryWidget::onSortOrderChanged()
{
    if (!m_model)
        return;

    int sortValue = m_sortComboBox->currentData().toInt();
    PhotoModel::SortOrder order = static_cast<PhotoModel::SortOrder>(sortValue);
    m_model->setSortOrder(order);

    qDebug() << "Sort order changed to:"
             << (order == PhotoModel::NewestFirst ? "Newest First"
                                                  : "Oldest First");
}

PhotoModel::FilterType GalleryWidget::getCurrentFilterType() const
{
    int filterValue = m_filterComboBox->currentData().toInt();
    return static_cast<PhotoModel::FilterType>(filterValue);
}

void GalleryWidget::onFilterChanged()
{
    if (!m_model)
        return;

    PhotoModel::FilterType filter = getCurrentFilterType();
    m_model->setFilterType(filter);

    QString filterName = m_filterComboBox->currentText();
    qDebug() << "Filter changed to:" << filterName;
}

void GalleryWidget::onExportSelected()
{
    // if we are exporting from album selection view
    if (m_loadingWidget->currentWidget() == m_albumSelectionWidget) {

        QModelIndexList selectedIndexes =
            m_albumListView->selectionModel()->selectedIndexes();
        // QStringList filePaths =
        // m_albumModel->getSelectedFilePaths(selectedIndexes);

        QStringList paths;
        for (const QModelIndex &index : selectedIndexes) {
            if (index.isValid() &&
                index.row() < m_albumListView->model()->rowCount()) {
                paths.append(index.data(Qt::UserRole).toString());
            } else {
                qDebug() << "Invalid index in selection:" << index;
            }
        }

        auto *exportAlbum = new ExportAlbum(m_device, paths, this);
        exportAlbum->show();
        return;
    }

    if (!m_model || !m_listView->selectionModel()->hasSelection()) {
        QMessageBox::information(this, "No Selection",
                                 "Please select photos to export.");
        return;
    }

    QModelIndexList selectedIndexes =
        m_listView->selectionModel()->selectedIndexes();
    QStringList filePaths = m_model->getSelectedFilePaths(selectedIndexes);

    if (filePaths.isEmpty()) {
        QMessageBox::information(this, "No Items",
                                 "No valid items selected for export.");
        return;
    }

    QString exportDir = selectExportDirectory();
    if (exportDir.isEmpty()) {
        return;
    }

    QList<QString> exportItems;
    for (const QString &filePath : filePaths) {
        exportItems.append(filePath);
    }

    qDebug() << "Starting export of selected files:" << exportItems.size()
             << "items to" << exportDir;

    IOManagerClient::sharedInstance()->startExport(
        m_device, exportItems, exportDir, "Exporting from gallery");
}

void GalleryWidget::onExportAll()
{
    // if we are exporting from album selection view
    if (m_loadingWidget->currentWidget() == m_albumSelectionWidget) {

        // get all available albums
        QStringList paths;
        for (int row = 0; row < m_albumListView->model()->rowCount(); ++row) {
            QModelIndex index = m_albumListView->model()->index(row, 0);
            if (index.isValid()) {
                paths.append(index.data(Qt::UserRole).toString());
            }
        }

        if (paths.isEmpty()) {
            QMessageBox::information(this, "No Albums",
                                     "No albums available for export.");
            return;
        }

        auto *exportAlbum = new ExportAlbum(m_device, paths, this);
        exportAlbum->show();
        return;
    }

    if (!m_model)
        return;

    QList<QString> exportItems = m_model->getFilteredFilePaths();

    if (exportItems.isEmpty()) {
        QMessageBox::information(this, "No Items", "No items to export.");
        return;
    }
    QString message =
        QString("Export all %1 items currently shown?").arg(exportItems.size());
    int reply = QMessageBox::question(this, "Export All", message,
                                      QMessageBox::Yes | QMessageBox::No,
                                      QMessageBox::No);

    if (reply != QMessageBox::Yes) {
        return;
    }

    QString exportDir = selectExportDirectory();
    if (exportDir.isEmpty()) {
        return;
    }

    qDebug() << "Starting export of:" << exportItems.size() << "items to"
             << exportDir;

    IOManagerClient::sharedInstance()->startExport(
        m_device, exportItems, exportDir, "Exporting from gallery");
}

QString GalleryWidget::selectExportDirectory()
{
    QString defaultDir =
        QStandardPaths::writableLocation(QStandardPaths::PicturesLocation);

    QString selectedDir = QFileDialog::getExistingDirectory(
        this, "Select Export Directory", defaultDir,
        QFileDialog::ShowDirsOnly | QFileDialog::DontResolveSymlinks | QFileDialog::DontUseNativeDialog);

    return selectedDir;
}

void GalleryWidget::setupAlbumSelectionView()
{
    m_albumSelectionWidget = new QWidget();
    QVBoxLayout *layout = new QVBoxLayout(m_albumSelectionWidget);
    layout->setContentsMargins(0, 0, 0, 0);
    // Add instructions label
    QLabel *instructionLabel = new QLabel("Select a photo album:");
    QFont instructionFont = instructionLabel->font();
    instructionFont.setWeight(QFont::Bold);
    instructionLabel->setFont(instructionFont);
    layout->addWidget(instructionLabel);

    m_albumListView = new QListView();
    m_albumListView->setViewMode(QListView::IconMode);
    m_albumListView->setFlow(QListView::LeftToRight);
    m_albumListView->setWrapping(true);
    m_albumListView->setResizeMode(QListView::Adjust);
    m_albumListView->setIconSize(QSize(200, 230));
    m_albumListView->setGridSize(QSize(210, 260));

    m_albumListView->setSelectionMode(QAbstractItemView::ExtendedSelection);
    m_albumListView->setUniformItemSizes(true);

    m_albumListView->setStyleSheet("QListView { "
                                   "    border-top: 1px solid #c1c1c1ff; "
                                   "    background-color: transparent; "
                                   "    border-radius: 0px;"
                                   "    padding: 0px;"
                                   "    padding-top: 5px;"
                                   "} "
                                   "QListView::item { "
                                   "    width: 200px; "
                                   "    height: 270px; "
                                   "    margin: 2px; "
                                   "}");

    layout->addWidget(m_albumListView);

    m_loadingWidget->setupContentWidget(m_albumSelectionWidget);

    connect(m_albumListView, &QListView::doubleClicked, this,
            [this](const QModelIndex &index) {
                if (!index.isValid())
                    return;
                QString albumPath = index.data(Qt::UserRole).toString();
                onAlbumSelected(albumPath);
            });
}

void GalleryWidget::setupPhotoGalleryView()
{
    m_photoGalleryWidget = new QWidget();
    QVBoxLayout *layout = new QVBoxLayout(m_photoGalleryWidget);
    layout->setContentsMargins(0, 0, 0, 0);

    // Create list view for photos
    m_listView = new QListView();
    m_listView->setViewMode(QListView::IconMode);
    m_listView->setFlow(QListView::LeftToRight);
    m_listView->setWrapping(true);
    m_listView->setResizeMode(QListView::Adjust);
    m_listView->setIconSize(QSize(200, 230));
    m_listView->setGridSize(QSize(210, 260));
    m_listView->setSelectionMode(QAbstractItemView::ExtendedSelection);
    m_listView->setUniformItemSizes(true);
    m_listView->setContextMenuPolicy(Qt::CustomContextMenu);

    m_listView->setStyleSheet("QListView { "
                              "    border-top: 1px solid #c1c1c1ff; "
                              "    background-color: transparent; "
                              "    border-radius: 0px;"
                              "    padding: 0px;"
                              "    padding-top: 5px;"
                              "} "
                              "QListView::item { "
                              "    width: 200px; "
                              "    height: 270px; "
                              "    margin: 2px; "
                              "}");

    layout->addWidget(m_listView);

    // Add the photo gallery widget to stacked widget
    m_loadingWidget->setupAditionalWidget(m_photoGalleryWidget);

    // Connect double-click to open preview dialog
    connect(m_listView, &QListView::doubleClicked, this,
            [this](const QModelIndex &index) {
                if (!index.isValid())
                    return;

                QString filePath =
                    m_model->data(index, Qt::UserRole).toString();
                if (filePath.isEmpty())
                    return;

                qDebug() << "Opening preview for" << filePath;
                auto *previewDialog = new MediaPreviewDialog(
                    m_device, filePath, std::nullopt, false, this);
                previewDialog->show();
            });

    connect(m_listView, &QListView::customContextMenuRequested, this,
            &GalleryWidget::onPhotoContextMenu);

    m_albumModel = new QStandardItemModel(this);
}

void GalleryWidget::onError()
{
    m_loadingWidget->showError();
    QMessageBox::warning(this, "Error",
                         "Could not access DCIM directory on device.");
    return;
}

void GalleryWidget::onAlbumListLoaded(const QList<QString> &dcimTree)
{
    qDebug() << "Albums loaded:" << dcimTree.size();
    if (dcimTree.isEmpty()) {
        m_loadingWidget->showError("No albums found on device");
        return;
    }

    m_albumModel->clear();

    for (const QString &albumName : dcimTree) {
        auto *item = new QStandardItem(albumName);
        QString fullPath = QString("/DCIM/%1").arg(albumName);
        item->setData(fullPath, Qt::UserRole);

        item->setIcon(QIcon(":/resources/icons/"
                            "MaterialSymbolsLightImageOutlineSharp.png"));
        m_albumModel->appendRow(item);

        loadAlbumThumbnailAsync(fullPath, item);
    }

    m_albumListView->setModel(m_albumModel);
    m_loadingWidget->stop();
    m_loadingWidget->switchToWidget(m_albumSelectionWidget);
    m_exportAllButton->setEnabled(m_albumModel->rowCount() > 0);

    connect(m_albumListView->selectionModel(),
            &QItemSelectionModel::selectionChanged, this, [this]() {
                bool hasSelection =
                    m_albumListView->selectionModel()->hasSelection();
                m_exportSelectedButton->setEnabled(hasSelection);
            });
}

void GalleryWidget::onAlbumSelected(const QString &albumPath)
{
    m_currentAlbumPath = albumPath;

    // Create model if not exists
    if (!m_model) {
        m_model = new PhotoModel(m_device, getCurrentFilterType(), this);
        m_listView->setModel(m_model);

        connect(m_model, &PhotoModel::albumPathSet, this, [this]() {
            // Switch to photo gallery view once album is loaded
            m_loadingWidget->stop(false);
            m_loadingWidget->switchToWidget(m_photoGalleryWidget);
            // Enable controls and show back button
            setControlsEnabled(true);
            m_backButton->show();
        });

        connect(m_model, &PhotoModel::albumPathSetFailed, this, [this]() {
            m_loadingWidget->stop(false);
            m_backButton->show();
            m_loadingWidget->showError("Failed to load album");
        });

        // Update export button states based on selection
        connect(m_listView->selectionModel(),
                &QItemSelectionModel::selectionChanged, this, [this]() {
                    bool hasSelection =
                        m_listView->selectionModel()->hasSelection();
                    m_exportSelectedButton->setEnabled(hasSelection);
                });
    }

    // Set album path and load photos
    m_model->setAlbumPath(albumPath);

    m_loadingWidget->showLoading();
}

void GalleryWidget::onBackToAlbums()
{
    // Switch back to album selection view
    m_loadingWidget->switchToWidget(m_albumSelectionWidget);

    if (m_model) {
        m_model->clear();
    }

    // Disable controls and hide back button
    setControlsEnabled(false);
    m_backButton->hide();
    // Clear current album path
    m_currentAlbumPath.clear();
}

void GalleryWidget::setControlsEnabled(bool enabled)
{
    m_sortComboBox->setEnabled(enabled);
    m_filterComboBox->setEnabled(enabled);

    const bool hasSelection = m_listView && m_listView->selectionModel() &&
                              m_listView->selectionModel()->hasSelection();

    m_exportSelectedButton->setEnabled(enabled && hasSelection);
}

/*
    FIXME: this needs to be refactored once we
    figure out how to query Photos.sqlite
    Check out:
    https://github.com/ScottKjr3347/iOS_Local_PL_Photos.sqlite_Queries
*/
QImage
GalleryWidget::loadAlbumThumbnail(const QString &albumPath,
                                  std::shared_ptr<iDescriptorDevice> device)
{
    if (QCoreApplication::closingDown() || !QGuiApplication::instance()) {
        return {};
    }

    QList<QString> albumTree = device->afc_backend->list_dir(albumPath);
    if (albumTree.isEmpty()) {
        qDebug() << "Failed to read album directory:" << albumPath;
        return {};
    }

    QString firstImagePath;
    for (const QString &fileName : albumTree) {
        bool isDir =
            device->afc_backend->is_directory((albumPath + "/" + fileName));
        if (!isDir && (fileName.endsWith(".JPG", Qt::CaseInsensitive) ||
                       fileName.endsWith(".PNG", Qt::CaseInsensitive) ||
                       fileName.endsWith(".HEIC", Qt::CaseInsensitive))) {
            firstImagePath = albumPath + "/" + fileName;
            break;
        }
    }

    if (firstImagePath.isEmpty()) {
        return {};
    }

    QByteArray imageData = device->afc_backend->file_to_buffer(firstImagePath);

    QImage thumbnail;
    if (firstImagePath.endsWith(".HEIC", Qt::CaseInsensitive)) {
        thumbnail = load_heic(imageData);
    } else {
        thumbnail.loadFromData(imageData);
    }

    return thumbnail;
}

void GalleryWidget::loadAlbumThumbnailAsync(const QString &albumPath,
                                            QStandardItem *item)
{
    Q_UNUSED(item);

    auto *watcher = new QFutureWatcher<QImage>(this);
    const auto device = m_device;

    connect(watcher, &QFutureWatcher<QImage>::finished, this,
            [this, watcher, albumPath]() {
                const QImage result = watcher->result();
                watcher->deleteLater();

                if (result.isNull() || !m_albumModel ||
                    QCoreApplication::closingDown() ||
                    !QGuiApplication::instance()) {
                    return;
                }

                for (int row = 0; row < m_albumModel->rowCount(); ++row) {
                    QModelIndex idx = m_albumModel->index(row, 0);
                    if (idx.data(Qt::UserRole).toString() == albumPath) {
                        if (auto *it = m_albumModel->itemFromIndex(idx)) {
                            it->setIcon(QIcon(QPixmap::fromImage(result)));
                        }
                        break;
                    }
                }
            });

    QFuture<QImage> future = QtConcurrent::run([albumPath, device]() {
        return loadAlbumThumbnail(albumPath, device);
    });

    watcher->setFuture(future);
}

void GalleryWidget::onPhotoContextMenu(const QPoint &pos)
{
    QModelIndex index = m_listView->indexAt(pos);
    if (!index.isValid()) {
        return;
    }

    // Make sure the item is selected
    if (!m_listView->selectionModel()->isSelected(index)) {
        m_listView->selectionModel()->select(
            index, QItemSelectionModel::ClearAndSelect);
    }

    QMenu contextMenu(this);
    QAction *previewAction = contextMenu.addAction("Preview");
    contextMenu.addSeparator();
    QAction *exportAction = contextMenu.addAction("Export");

    exportAction->setEnabled(m_listView->selectionModel()->hasSelection());

    connect(previewAction, &QAction::triggered, this, [this, index]() {
        // Re-use the double-click logic
        if (!index.isValid())
            return;

        QString filePath = m_model->data(index, Qt::UserRole).toString();
        if (filePath.isEmpty())
            return;

        qDebug() << "Opening preview for" << filePath;
        auto *previewDialog = new MediaPreviewDialog(m_device, filePath);
        previewDialog->show();
    });

    connect(exportAction, &QAction::triggered, this,
            &GalleryWidget::onExportSelected);

    contextMenu.exec(m_listView->viewport()->mapToGlobal(pos));
}

void GalleryWidget::handleImport()
{
    QString defaultDir = QStandardPaths::writableLocation(QStandardPaths::PicturesLocation);
    
    QStringList filePaths = QFileDialog::getOpenFileNames(
        this, "Select Photos to Import",
        defaultDir,
        "Images (*.jpg *.jpeg *.png *.heic);;All Files (*)",
        nullptr,
        QFileDialog::DontUseNativeDialog);

    if (filePaths.isEmpty()) {
        return;
    }

    qDebug() << "Selected files for import:" << filePaths;

    PhotoImportDialog dialog(filePaths, this);
    dialog.exec();
}

GalleryWidget::~GalleryWidget()
{
    qDebug() << "GalleryWidget destructor called";
}
