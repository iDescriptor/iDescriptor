// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::constants::{
    ALBUM_CONTENTS_QUERY_TEMPLATE, FAVS_ALBUM_ID, FAVS_ALBUM_QUERY, FAVS_QUERY,
    GALLERY_TOTAL_SIZE_QUERY, HIDDEN_ALBUM_ID, HIDDEN_ALBUM_QUERY, HIDDEN_QUERY,
    IOS_6_ALBUM_CONTENTS_QUERY_TEMPLATE, IOS_6_ALBUM_QUERY_STATEMENT, IOS_6_RECENTS_ALBUM_QUERY,
    IOS_6_RECENTS_QUERY, IOS_6_VISIBLE_ASSET_PREDICATE, IOS_12_ALBUM_QUERY_STATEMENT,
    IOS_15_ALBUM_QUERY_STATEMENT, IOS_26_ALBUM_QUERY_STATEMENT, PHOTOS_SQLITE_REMOTE_PATH,
    PHOTOS_SQLITE_SHM_REMOTE_PATH, PHOTOS_SQLITE_WAL_REMOTE_PATH, RECENTLY_DELETED_ALBUM_ID,
    RECENTLY_DELETED_ALBUM_QUERY, RECENTLY_DELETED_QUERY, RECENTS_ALBUM_ID, RECENTS_ALBUM_QUERY,
    RECENTS_QUERY, SQLITE_GALLERY_PROVIDER_NAME, SQLITE_VFS_GALLERY_PROVIDER_NAME,
};
use crate::gallery::{
    GalleryAlbum, GalleryFuture, GalleryMediaFilter, GalleryProvider, export_afc_file,
    matches_media_filter,
};
use crate::gallery_sqlite_vfs::{GalleryVfsRegistration, open_gallery_vfs_connection};
use crate::utils::TempDirGuard;
use ::log::{debug, info, warn};
use anyhow::{Context, anyhow};
use idevice::IdeviceError;
use idevice::afc::AfcClient;
use idevice::afc::errors::AfcError;
use idevice::provider::IdeviceProvider;
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum SnapshotFile {
    Database,
    Wal,
    Shm,
}

impl SnapshotFile {
    const ALL: [Self; 3] = [Self::Database, Self::Wal, Self::Shm];

    fn remote_path(self) -> &'static str {
        match self {
            Self::Database => PHOTOS_SQLITE_REMOTE_PATH,
            Self::Wal => PHOTOS_SQLITE_WAL_REMOTE_PATH,
            Self::Shm => PHOTOS_SQLITE_SHM_REMOTE_PATH,
        }
    }

    fn local_name(self) -> &'static str {
        match self {
            Self::Database => "Photos.sqlite",
            Self::Wal => "Photos.sqlite-wal",
            Self::Shm => "Photos.sqlite-shm",
        }
    }

    fn is_required(self) -> bool {
        self == Self::Database
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RemoteFileMetadata {
    size: usize,
    modified: String,
}

type SnapshotMetadata = HashMap<SnapshotFile, Option<RemoteFileMetadata>>;

struct SqliteProviderState {
    connection: Option<Connection>,
    vfs_registration: Option<GalleryVfsRegistration>,
    schema: GallerySchema,
    committed_metadata: SnapshotMetadata,
}

#[derive(Clone, Debug)]
struct GallerySchema {
    kind: GallerySchemaKind,
    asset_table: &'static str,
    relationship_table: String,
    album_column: String,
    asset_column: String,
    order_column: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GallerySchemaKind {
    Ios6,
    Ios12,
    Modern,
}

impl GallerySchema {
    fn supports_smart_albums(&self) -> bool {
        self.kind != GallerySchemaKind::Ios6
    }

    fn recents_query(&self, most_recent_first: bool) -> String {
        let template = if self.kind == GallerySchemaKind::Ios6 {
            IOS_6_RECENTS_QUERY
        } else {
            RECENTS_QUERY
        };
        self.query(&sqlite_ordered_query(template, most_recent_first))
    }

    fn album_contents_query(&self, most_recent_first: bool) -> String {
        let template = if self.kind == GallerySchemaKind::Ios6 {
            IOS_6_ALBUM_CONTENTS_QUERY_TEMPLATE
        } else {
            ALBUM_CONTENTS_QUERY_TEMPLATE
        };
        self.query(&format!(
            "{template} ORDER BY ZASSET.Z_PK {}",
            sqlite_order_direction(most_recent_first)
        ))
    }

    fn query(&self, template: &str) -> String {
        // Substitute the historical identifiers used by the shared templates.
        // Identifiers come only from validated SQLite schema names, never user input.
        template
            // FIXME: this can be problematic in future
            .replace("{visible}", IOS_6_VISIBLE_ASSET_PREDICATE)
            .replace("ZASSET", self.asset_table)
            .replace("Z_FOK_3ASSETS", &self.order_column)
            .replace("Z_3ASSETS", &self.asset_column)
            .replace("{table}", &self.relationship_table)
            .replace("{album}", &self.album_column)
    }
}

#[derive(Clone)]
enum SqliteProviderSource {
    Snapshot {
        temp_dir: PathBuf,
    },
    Vfs {
        provider: Arc<Mutex<Box<dyn IdeviceProvider>>>,
    },
}

pub struct SqliteGalleryProvider {
    state: Arc<Mutex<SqliteProviderState>>,
    refresh_lock: Arc<Mutex<()>>,
    afc: Arc<Mutex<AfcClient>>,
    source: SqliteProviderSource,
    ios_version: u32,
    name: String,
}

impl GalleryProvider for SqliteGalleryProvider {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn read_albums(&self) -> GalleryFuture<(Vec<GalleryAlbum>, i32)> {
        let state = self.state.clone();
        let ios_ver = self.ios_version;

        Box::pin(async move {
            debug!("Reading via Sqlite provider for ios {ios_ver}");
            let state = state.lock().await;
            let conn = state
                .connection
                .as_ref()
                .context("SQLite gallery connection is closed")?;
            tokio::task::block_in_place(|| {
                read_albums_from_connection(conn, ios_ver, &state.schema)
            })
        })
    }

    fn reload(&self) -> GalleryFuture<(Vec<GalleryAlbum>, i32)> {
        let state = self.state.clone();
        let refresh_lock = self.refresh_lock.clone();
        let afc = self.afc.clone();
        let source = self.source.clone();
        let ios_version = self.ios_version;

        Box::pin(async move {
            let _refresh_guard = refresh_lock.lock().await;
            if let SqliteProviderSource::Vfs { provider } = source {
                let mut state = state.lock().await;
                tokio::task::block_in_place(|| close_connection(&mut state))?;
                state.vfs_registration = None;

                let (connection, registration) = open_gallery_vfs_connection(afc, provider).await?;
                let prepared = tokio::task::block_in_place(|| {
                    (|| -> anyhow::Result<_> {
                        let schema = validate_vfs_connection(&connection)?;
                        let albums =
                            read_albums_from_connection(&connection, ios_version, &schema)?;
                        Ok((schema, albums))
                    })()
                });
                let (schema, albums) = match prepared {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        drop(connection);
                        drop(registration);
                        return Err(error);
                    }
                };

                state.connection = Some(connection);
                state.vfs_registration = Some(registration);
                state.schema = schema;
                return Ok(albums);
            }

            let SqliteProviderSource::Snapshot { temp_dir } = source else {
                unreachable!();
            };
            let remote_metadata = {
                let mut afc = afc.lock().await;
                read_remote_snapshot_metadata(&mut afc).await?
            };

            let mut state = state.lock().await;
            close_connection(&mut state)?;

            let mut afc = afc.lock().await;
            for snapshot_file in SnapshotFile::ALL {
                let local_path = temp_dir.join(snapshot_file.local_name());
                let remote_file_metadata = remote_metadata.get(&snapshot_file).cloned().flatten();

                match remote_file_metadata {
                    Some(_) => {
                        if snapshot_file_changed(
                            &state.committed_metadata,
                            &remote_metadata,
                            snapshot_file,
                        ) {
                            info!(
                                "Reloading changed gallery snapshot file {}",
                                snapshot_file.remote_path()
                            );
                            export_afc_file(&mut afc, snapshot_file.remote_path(), &local_path)
                                .await
                                .with_context(|| {
                                    format!("Failed to refresh {}", snapshot_file.remote_path())
                                })?;
                        }
                    }
                    None => {
                        if snapshot_file.is_required() {
                            anyhow::bail!("Required Photos.sqlite is missing from the device");
                        }
                        if local_path.exists() {
                            tokio::fs::remove_file(&local_path).await.with_context(|| {
                                format!("Failed to remove stale {}", local_path.display())
                            })?;
                        }
                    }
                }
            }
            drop(afc);

            let database_path = temp_dir.join(SnapshotFile::Database.local_name());
            let (connection, schema) = open_and_validate_connection(&database_path)?;
            let albums = read_albums_from_connection(&connection, ios_version, &schema)?;

            state.connection = Some(connection);
            state.vfs_registration = None;
            state.schema = schema;
            state.committed_metadata = remote_metadata;
            Ok(albums)
        })
    }

    fn query_album(
        &self,
        id: i32,
        media_filter: GalleryMediaFilter,
        most_recent_first: bool,
    ) -> GalleryFuture<Vec<String>> {
        let state = self.state.clone();

        Box::pin(async move {
            if matches!(
                id,
                FAVS_ALBUM_ID | HIDDEN_ALBUM_ID | RECENTLY_DELETED_ALBUM_ID
            ) && !state.lock().await.schema.supports_smart_albums()
            {
                return Ok(Vec::new());
            }
            match id {
                FAVS_ALBUM_ID => query_favs_album(state, media_filter, most_recent_first).await,
                RECENTS_ALBUM_ID => {
                    query_recents_album(state, media_filter, most_recent_first).await
                }
                RECENTLY_DELETED_ALBUM_ID => {
                    query_recently_deleted_album(state, media_filter, most_recent_first).await
                }
                HIDDEN_ALBUM_ID => query_hidden_album(state, media_filter, most_recent_first).await,
                _ => query_sqlite_album(state, id, media_filter, most_recent_first).await,
            }
        })
    }

    fn query_gallery_size(&self) -> GalleryFuture<u64> {
        let state = self.state.clone();
        Box::pin(async move {
            let state = state.lock().await;
            let conn = state
                .connection
                .as_ref()
                .context("SQLite gallery connection is closed")?;
            let total_size: i64 = tokio::task::block_in_place(|| {
                conn.query_row(GALLERY_TOTAL_SIZE_QUERY, [], |r| r.get(0))
            })?;
            u64::try_from(total_size).context("Gallery size cannot be negative")
        })
    }
}

impl Drop for SqliteGalleryProvider {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.try_lock() {
            let _ = close_connection(&mut state);
            state.vfs_registration = None;
        }

        if let SqliteProviderSource::Snapshot { temp_dir } = &self.source {
            if let Err(e) = std::fs::remove_dir_all(temp_dir) {
                println!("Failed to remove temp gallery database dir: {}", e);
            }
        }
    }
}

pub async fn build_sqlite_provider(
    afc: Arc<Mutex<AfcClient>>,
    ios_version: u32,
) -> anyhow::Result<Arc<dyn GalleryProvider>> {
    let temp_dir =
        std::env::temp_dir().join(format!("idescriptor-photos-db-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await?;
    // this is so that if we fail to export the db, we don't leave a temp dir behind
    let temp_dir_guard = TempDirGuard::new(temp_dir);

    let remote_metadata = {
        let mut afc_guard = afc.lock().await;
        let metadata = read_remote_snapshot_metadata(&mut afc_guard).await?;
        for snapshot_file in SnapshotFile::ALL {
            match metadata.get(&snapshot_file).cloned().flatten() {
                Some(_) => {
                    export_afc_file(
                        &mut afc_guard,
                        snapshot_file.remote_path(),
                        &temp_dir_guard.path().join(snapshot_file.local_name()),
                    )
                    .await
                    .with_context(|| format!("Failed to export {}", snapshot_file.remote_path()))?;
                }
                None if snapshot_file.is_required() => {
                    anyhow::bail!("Required Photos.sqlite is missing from the device");
                }
                None => {}
            }
        }
        metadata
    };

    let gallery_db_path = temp_dir_guard
        .path()
        .join(SnapshotFile::Database.local_name());
    let (connection, schema) = open_and_validate_connection(&gallery_db_path)?;

    let temp_dir = temp_dir_guard.keep();
    Ok(Arc::new(SqliteGalleryProvider {
        state: Arc::new(Mutex::new(SqliteProviderState {
            connection: Some(connection),
            vfs_registration: None,
            schema,
            committed_metadata: remote_metadata,
        })),
        refresh_lock: Arc::new(Mutex::new(())),
        afc,
        source: SqliteProviderSource::Snapshot { temp_dir },
        ios_version,
        name: SQLITE_GALLERY_PROVIDER_NAME.into(),
    }))
}

pub async fn build_sqlite_vfs_provider(
    afc: Arc<Mutex<AfcClient>>,
    provider: Arc<Mutex<Box<dyn IdeviceProvider>>>,
    ios_version: u32,
) -> anyhow::Result<Arc<dyn GalleryProvider>> {
    let (connection, registration) =
        open_gallery_vfs_connection(afc.clone(), provider.clone()).await?;
    let schema = match tokio::task::block_in_place(|| validate_vfs_connection(&connection)) {
        Ok(schema) => schema,
        Err(error) => {
            drop(connection);
            drop(registration);
            return Err(error);
        }
    };

    Ok(Arc::new(SqliteGalleryProvider {
        state: Arc::new(Mutex::new(SqliteProviderState {
            connection: Some(connection),
            vfs_registration: Some(registration),
            schema,
            committed_metadata: SnapshotMetadata::new(),
        })),
        refresh_lock: Arc::new(Mutex::new(())),
        afc,
        source: SqliteProviderSource::Vfs { provider },
        ios_version,
        name: SQLITE_VFS_GALLERY_PROVIDER_NAME.into(),
    }))
}

async fn read_remote_snapshot_metadata(afc: &mut AfcClient) -> anyhow::Result<SnapshotMetadata> {
    let mut metadata = SnapshotMetadata::new();
    for snapshot_file in SnapshotFile::ALL {
        match afc.get_file_info(snapshot_file.remote_path()).await {
            Ok(info) => {
                metadata.insert(
                    snapshot_file,
                    Some(RemoteFileMetadata {
                        size: info.size,
                        modified: info.modified.to_string(),
                    }),
                );
            }
            Err(err) if snapshot_file.is_required() || !is_missing_file_error(&err) => {
                return Err(err)
                    .with_context(|| format!("Failed to stat {}", snapshot_file.remote_path()));
            }
            Err(err) => {
                warn!(
                    "Optional gallery snapshot file {} is unavailable: {}",
                    snapshot_file.remote_path(),
                    err
                );
                metadata.insert(snapshot_file, None);
            }
        }
    }
    Ok(metadata)
}

fn is_missing_file_error(error: &IdeviceError) -> bool {
    matches!(
        error,
        IdeviceError::NotFound | IdeviceError::Afc(AfcError::ObjectNotFound)
    )
}

fn snapshot_file_changed(
    committed: &SnapshotMetadata,
    current: &SnapshotMetadata,
    snapshot_file: SnapshotFile,
) -> bool {
    committed.get(&snapshot_file).cloned().flatten()
        != current.get(&snapshot_file).cloned().flatten()
}

fn close_connection(state: &mut SqliteProviderState) -> anyhow::Result<()> {
    let Some(connection) = state.connection.take() else {
        return Ok(());
    };

    match connection.close() {
        Ok(()) => Ok(()),
        Err((connection, err)) => {
            state.connection = Some(connection);
            Err(anyhow!("Failed to close SQLite gallery connection: {err}"))
        }
    }
}

fn open_and_validate_connection(path: &Path) -> anyhow::Result<(Connection, GallerySchema)> {
    let connection = Connection::open(path)
        .with_context(|| format!("Failed to open gallery database {}", path.display()))?;
    let quick_check: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .context("Failed to validate refreshed gallery database")?;
    if quick_check != "ok" {
        anyhow::bail!("Gallery database validation failed: {quick_check}");
    }

    let schema = discover_assets_table(&connection)?;
    Ok((connection, schema))
}

fn validate_vfs_connection(connection: &Connection) -> anyhow::Result<GallerySchema> {
    // TODO: do we need this?
    // connection
    //     .query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| {
    //         row.get::<_, i64>(0)
    //     })
    //     .context("Failed to read the gallery schema through the AFC VFS")?;
    discover_assets_table(connection)
}

fn discover_assets_table(connection: &Connection) -> anyhow::Result<GallerySchema> {
    /*
        We need to get the dynamic asset table name from the database.
        iOS seems to be bumping the version with every major iOS update.
    */
    let asset_table = if connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='ZASSET')",
        [],
        |row| row.get::<_, bool>(0),
    )? {
        "ZASSET"
    } else if connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='ZGENERICASSET')",
        [],
        |row| row.get::<_, bool>(0),
    )? {
        "ZGENERICASSET"
    } else {
        anyhow::bail!("Unsupported gallery schema: neither ZASSET nor ZGENERICASSET exists");
    };
    connection
        .prepare(&format!(
            "SELECT Z_PK, ZDIRECTORY, ZFILENAME FROM {asset_table} LIMIT 0"
        ))
        .context("Gallery asset table is missing required columns")?;
    let mut columns_statement = connection.prepare(&format!("PRAGMA table_info({asset_table})"))?;
    let columns = columns_statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let state_column_count = ["ZTRASHEDSTATE", "ZVISIBILITYSTATE", "ZHIDDEN", "ZFAVORITE"]
        .iter()
        .filter(|column| columns.iter().any(|name| name == **column))
        .count();
    let kind = match (asset_table, state_column_count) {
        ("ZGENERICASSET", 0) => {
            // Do not treat a partially missing modern schema as unfiltered iOS 6.
            connection.prepare("SELECT Z_ENT, ZKIND FROM ZGENERICASSET LIMIT 0")?;
            connection.prepare("SELECT ZKIND, ZKEYASSET, ZTITLE FROM ZGENERICALBUM LIMIT 0")?;
            let asset_entities: i64 = connection.query_row(
                "SELECT COUNT(*) FROM Z_PRIMARYKEY WHERE Z_NAME='Asset'",
                [],
                |row| row.get(0),
            )?;
            anyhow::ensure!(
                asset_entities == 1,
                "iOS 6 gallery schema has no unambiguous Asset entity"
            );
            GallerySchemaKind::Ios6
        }
        ("ZGENERICASSET", 4) => GallerySchemaKind::Ios12,
        ("ZASSET", 4) => GallerySchemaKind::Modern,
        _ => {
            anyhow::bail!("Unsupported gallery schema: missing favorite/hidden/trash state columns")
        }
    };

    let mut statement =
        connection.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;
    let tables = statement.query_map([], |row| row.get::<_, String>(0))?;
    let table_pattern = regex::Regex::new(r"^Z_\d+ASSETS$")?;

    for table in tables {
        let table_name = table?;
        if !table_pattern.is_match(&table_name) {
            continue;
        }

        let prefix = table_name
            .strip_suffix("ASSETS")
            .ok_or_else(|| anyhow!("Couldn't derive the album column from {table_name}"))?;
        let album_column = format!("{prefix}ALBUMS");
        let mut columns_statement =
            connection.prepare(&format!("PRAGMA table_info({table_name})"))?;
        let columns = columns_statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if !columns.contains(&album_column) {
            continue;
        }
        let mut asset_columns = columns
            .iter()
            .filter(|column| table_pattern.is_match(column));
        let Some(asset_column) = asset_columns.next().cloned() else {
            continue;
        };
        if asset_columns.next().is_some() {
            anyhow::bail!("Ambiguous gallery asset columns in {table_name}");
        }
        let order_column = asset_column.replacen("Z_", "Z_FOK_", 1);
        if !columns.contains(&order_column) {
            anyhow::bail!("Gallery relationship table {table_name} is missing {order_column}");
        }
        info!(
            "Found gallery relationship table {} with album column {}",
            table_name, album_column
        );
        info!(
            "Using {kind:?} gallery schema: asset table {asset_table}, membership column {asset_column}"
        );
        return Ok(GallerySchema {
            kind,
            asset_table,
            relationship_table: table_name,
            album_column,
            asset_column,
            order_column,
        });
    }

    Err(anyhow!(
        "Couldn't find the gallery assets relationship table"
    ))
}

fn read_albums_from_connection(
    connection: &Connection,
    ios_version: u32,
    schema: &GallerySchema,
) -> anyhow::Result<(Vec<GalleryAlbum>, i32)> {
    let mut albums = Vec::new();
    let mut failed_albums_count = 0;

    let (fname, fdir, count) = explore_recents_album(connection, schema)?;
    albums.push(GalleryAlbum {
        id: RECENTS_ALBUM_ID,
        name: String::from("Recents"),
        item_count: count,
        preview_path: join_device_path(&fdir, &fname),
    });

    let (other_albums, other_failed_albums_count) =
        explore_other_albums(connection, ios_version, schema)?;
    albums.extend(other_albums);
    failed_albums_count += other_failed_albums_count;

    if !schema.supports_smart_albums() {
        return Ok((albums, failed_albums_count));
    }

    let (fname, fdir, count) = explore_favs_album(connection, schema)?;
    albums.push(GalleryAlbum {
        id: FAVS_ALBUM_ID,
        name: String::from("Favorites"),
        item_count: count,
        preview_path: join_device_path(&fdir, &fname),
    });

    let (fname, fdir, count) = explore_hidden_album(connection, schema)?;
    albums.push(GalleryAlbum {
        id: HIDDEN_ALBUM_ID,
        name: String::from("Hidden"),
        item_count: count,
        preview_path: join_device_path(&fdir, &fname),
    });

    let (fname, fdir, count) = explore_recently_deleted(connection, schema)?;
    albums.push(GalleryAlbum {
        id: RECENTLY_DELETED_ALBUM_ID,
        name: String::from("Recently Deleted"),
        item_count: count,
        preview_path: join_device_path(&fdir, &fname),
    });

    debug!("Failed to read {failed_albums_count} albums");
    Ok((albums, failed_albums_count))
}

async fn query_sqlite_album(
    state: Arc<Mutex<SqliteProviderState>>,
    id: i32,
    media_filter: GalleryMediaFilter,
    most_recent_first: bool,
) -> anyhow::Result<Vec<String>> {
    let state = state.lock().await;
    tokio::task::block_in_place(|| {
        let connection = state
            .connection
            .as_ref()
            .context("SQLite gallery connection is closed")?;
        let mut paths = Vec::new();
        let query = state.schema.album_contents_query(most_recent_first);
        debug!("Executing query: {}", query);
        debug!("With album id: {}", id);
        let mut stmt = connection.prepare(&query)?;

        let row_iter = stmt.query_map([id], |r| {
            let fdir: String = r.get(0)?;
            let fname: String = r.get(1)?;
            Ok((fdir, fname))
        })?;

        for item in row_iter {
            let (fdir, fname) = item?;
            let path = join_device_path(&fdir, &fname);
            if matches_media_filter(&path, media_filter) {
                paths.push(path);
            }
        }

        Ok(paths)
    })
}

fn sqlite_order_direction(most_recent_first: bool) -> &'static str {
    if most_recent_first { "DESC" } else { "ASC" }
}

fn sqlite_ordered_query(query: &str, most_recent_first: bool) -> String {
    query.replace(
        "ORDER BY ZASSET.Z_PK DESC",
        &format!(
            "ORDER BY ZASSET.Z_PK {}",
            sqlite_order_direction(most_recent_first)
        ),
    )
}

async fn query_favs_album(
    state: Arc<Mutex<SqliteProviderState>>,
    media_filter: GalleryMediaFilter,
    most_recent_first: bool,
) -> anyhow::Result<Vec<String>> {
    let state = state.lock().await;
    tokio::task::block_in_place(|| {
        let connection = state
            .connection
            .as_ref()
            .context("SQLite gallery connection is closed")?;
        let mut paths = Vec::new();

        //favs album
        let query = state
            .schema
            .query(&sqlite_ordered_query(FAVS_QUERY, most_recent_first));
        let mut favs_stmt = connection.prepare(&query)?;

        let favs_iter = favs_stmt.query_map([], |r| {
            let fname: String = r.get(0)?;
            let fdir: String = r.get(1)?;
            Ok((fname, fdir))
        })?;

        for fav_item in favs_iter {
            let (fname, fdir) = fav_item?;
            let path = join_device_path(&fdir, &fname);
            if matches_media_filter(&path, media_filter) {
                paths.push(path);
            }
        }

        Ok(paths)
    })
}

async fn query_recents_album(
    state: Arc<Mutex<SqliteProviderState>>,
    media_filter: GalleryMediaFilter,
    most_recent_first: bool,
) -> anyhow::Result<Vec<String>> {
    let state = state.lock().await;
    tokio::task::block_in_place(|| {
        let connection = state
            .connection
            .as_ref()
            .context("SQLite gallery connection is closed")?;
        let mut paths = Vec::new();

        //recents album
        let query = state.schema.recents_query(most_recent_first);
        let mut recents_stmt = connection.prepare(&query)?;

        let recents_iter = recents_stmt.query_map([], |r| {
            let fname: String = r.get(0)?;
            let fdir: String = r.get(1)?;
            Ok((fname, fdir))
        })?;

        for recent_item in recents_iter {
            let (fname, fdir) = recent_item?;
            let path = join_device_path(&fdir, &fname);
            if matches_media_filter(&path, media_filter) {
                paths.push(path);
            }
        }

        Ok(paths)
    })
}

async fn query_hidden_album(
    state: Arc<Mutex<SqliteProviderState>>,
    media_filter: GalleryMediaFilter,
    most_recent_first: bool,
) -> anyhow::Result<Vec<String>> {
    let state = state.lock().await;
    tokio::task::block_in_place(|| {
        let connection = state
            .connection
            .as_ref()
            .context("SQLite gallery connection is closed")?;
        let mut paths = Vec::new();

        let query = state
            .schema
            .query(&sqlite_ordered_query(HIDDEN_QUERY, most_recent_first));
        let mut hidden_stmt = connection.prepare(&query)?;

        let hidden_iter = hidden_stmt.query_map([], |r| {
            let fname: String = r.get(0)?;
            let fdir: String = r.get(1)?;
            Ok((fname, fdir))
        })?;

        for hidden_item in hidden_iter {
            let (fname, fdir) = hidden_item?;
            let path = join_device_path(&fdir, &fname);
            if matches_media_filter(&path, media_filter) {
                paths.push(path);
            }
        }

        Ok(paths)
    })
}

async fn query_recently_deleted_album(
    state: Arc<Mutex<SqliteProviderState>>,
    media_filter: GalleryMediaFilter,
    most_recent_first: bool,
) -> anyhow::Result<Vec<String>> {
    let state = state.lock().await;
    tokio::task::block_in_place(|| {
        let connection = state
            .connection
            .as_ref()
            .context("SQLite gallery connection is closed")?;
        let mut paths = Vec::new();

        //recently deleted album
        let query = state.schema.query(&sqlite_ordered_query(
            RECENTLY_DELETED_QUERY,
            most_recent_first,
        ));
        let mut recently_deleted_stmt = connection.prepare(&query)?;

        let recently_deleted_iter = recently_deleted_stmt.query_map([], |r| {
            let fname: String = r.get(0)?;
            let fdir: String = r.get(1)?;
            Ok((fname, fdir))
        })?;

        for deleted_item in recently_deleted_iter {
            let (fname, fdir) = deleted_item?;
            let path = join_device_path(&fdir, &fname);
            if matches_media_filter(&path, media_filter) {
                paths.push(path);
            }
        }

        Ok(paths)
    })
}

fn join_device_path(dir: &str, file_name: &str) -> String {
    format!("{}/{}", dir.trim_end_matches('/'), file_name)
}

//used in init
fn explore_recents_album(
    conn: &Connection,
    schema: &GallerySchema,
) -> anyhow::Result<(String, String, i32)> {
    let template = if schema.kind == GallerySchemaKind::Ios6 {
        IOS_6_RECENTS_ALBUM_QUERY
    } else {
        RECENTS_ALBUM_QUERY
    };
    let mut recents_stmt = conn.prepare(&schema.query(template))?;

    let recents_row = recents_stmt
        .query_row([], |r| {
            let fname: String = r.get(0)?;
            let fdir: String = r.get(1)?;
            let count: i32 = r.get(2)?;
            Ok((fname, fdir, count))
        })
        .optional()?
        .unwrap_or_default();

    Ok(recents_row)
}

fn explore_favs_album(
    conn: &Connection,
    schema: &GallerySchema,
) -> anyhow::Result<(String, String, i32)> {
    let mut favs_stmt = conn.prepare(&schema.query(FAVS_ALBUM_QUERY))?;

    let favs_row = favs_stmt
        .query_row([], |r| {
            let fname: String = r.get(0)?;
            let fdir: String = r.get(1)?;
            let count: i32 = r.get(2)?;
            Ok((fname, fdir, count))
        })
        .optional()?
        .unwrap_or_default();

    Ok(favs_row)
}

fn explore_hidden_album(
    conn: &Connection,
    schema: &GallerySchema,
) -> anyhow::Result<(String, String, i32)> {
    let mut hidden_stmt = conn.prepare(&schema.query(HIDDEN_ALBUM_QUERY))?;

    let hidden_row = hidden_stmt
        .query_row([], |r| {
            let fname: String = r.get(0)?;
            let fdir: String = r.get(1)?;
            let count: i32 = r.get(2)?;
            Ok((fname, fdir, count))
        })
        .optional()?
        .unwrap_or_default();

    Ok(hidden_row)
}

fn explore_recently_deleted(
    conn: &Connection,
    schema: &GallerySchema,
) -> anyhow::Result<(String, String, i32)> {
    let mut recently_deleted_stmt = conn.prepare(&schema.query(RECENTLY_DELETED_ALBUM_QUERY))?;

    let recently_deleted_row = recently_deleted_stmt
        .query_row([], |r| {
            let fname: String = r.get(0)?;
            let fdir: String = r.get(1)?;
            let count: i32 = r.get(2)?;
            Ok((fname, fdir, count))
        })
        .optional()?
        .unwrap_or_default();

    Ok(recently_deleted_row)
}

fn explore_other_albums(
    conn: &Connection,
    ios_ver: u32,
    schema: &GallerySchema,
) -> anyhow::Result<(Vec<GalleryAlbum>, i32)> {
    let query = album_query(ios_ver, schema);

    let mut stmt = conn.prepare(&query)?;
    let rows_iter = stmt.query_map([], |row| {
        let album_id: i32 = row.get(0)?;
        let title: String = row.get(1)?;
        let item_count: i32 = row.get(2)?;
        let asset_dir: String = row.get(3)?;
        let asset_file_name: String = row.get(4)?;
        Ok((album_id, title, item_count, asset_dir, asset_file_name))
    })?;

    let mut albums = Vec::new();
    let mut failed_albums_count = 0;
    for row_res in rows_iter {
        match row_res {
            Ok((album_id, title, item_count, asset_dir, asset_file_name)) => {
                println!(
                    "Found album: {title} with {item_count} items, preview: {asset_dir}/{asset_file_name}"
                );
                albums.push(GalleryAlbum {
                    id: album_id,
                    name: title,
                    item_count,
                    preview_path: join_device_path(&asset_dir, &asset_file_name),
                });
            }
            Err(_) => failed_albums_count += 1,
        }
    }

    Ok((albums, failed_albums_count))
}

fn album_query(ios_version: u32, schema: &GallerySchema) -> String {
    if schema.kind == GallerySchemaKind::Ios6 {
        return schema.query(IOS_6_ALBUM_QUERY_STATEMENT);
    }
    if schema.kind == GallerySchemaKind::Ios12 {
        return schema.query(IOS_12_ALBUM_QUERY_STATEMENT);
    }
    if ios_version <= 15 {
        return IOS_15_ALBUM_QUERY_STATEMENT.to_string();
    }

    schema.query(IOS_26_ALBUM_QUERY_STATEMENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ios6_fixture() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("
            CREATE TABLE Z_PRIMARYKEY (Z_ENT INTEGER, Z_NAME TEXT);
            INSERT INTO Z_PRIMARYKEY VALUES (14, 'GenericAsset'), (15, 'Asset'), (16, 'WallpaperAsset');
            CREATE TABLE ZGENERICASSET (
                Z_PK INTEGER PRIMARY KEY, Z_ENT INTEGER, ZKIND INTEGER,
                ZDIRECTORY TEXT, ZFILENAME TEXT, ZFLAGGED INTEGER
            );
            INSERT INTO ZGENERICASSET VALUES
                (1, 15, 0, 'DCIM/100APPLE', 'one.JPG', 0),
                (2, 15, 1, 'DCIM/100APPLE', 'two.MOV', 1),
                (3, 16, 0, NULL, NULL, 0),
                (4, 16, 0, 'Wallpaper', 'wallpaper.JPG', 0),
                (5, 15, 0, 'DCIM/100APPLE', NULL, 0),
                (6, 15, 0, '', 'no-directory.JPG', 0),
                (7, 15, 0, 'DCIM/100APPLE', '', 0);
            CREATE TABLE ZGENERICALBUM (
                Z_PK INTEGER PRIMARY KEY, ZKIND INTEGER, ZTITLE TEXT,
                ZCACHEDCOUNT INTEGER, ZKEYASSET INTEGER
            );
            INSERT INTO ZGENERICALBUM VALUES
                (10, 1000, 'Camera Roll', 99, 1),
                (20, 2, 'Test album', 999, 3),
                (30, 1501, 'Wallpaper', 2, 4),
                (40, 2, 'Empty', 0, NULL);
            CREATE TABLE Z_8ASSETS (Z_8ALBUMS INTEGER, Z_14ASSETS INTEGER, Z_FOK_14ASSETS INTEGER);
            INSERT INTO Z_8ASSETS VALUES
                (10, 1, 0), (10, 2, 1), (10, 3, 2), (10, 4, 3), (10, 5, 4), (10, 6, 5), (10, 7, 6),
                (20, 1, 0), (20, 2, 1), (20, 3, 2), (20, 4, 3), (20, 5, 4),
                (30, 3, 0), (30, 4, 1);
            CREATE TABLE ZADDITIONALASSETATTRIBUTES (ZORIGINALFILESIZE INTEGER);
            INSERT INTO ZADDITIONALASSETATTRIBUTES VALUES (100), (200);
        ").unwrap();
        connection
    }

    #[test]
    fn ios6_album_profile_filters_system_entities_and_unsupported_smart_albums() {
        let connection = ios6_fixture();
        let schema = discover_assets_table(&connection).unwrap();
        assert_eq!(schema.kind, GallerySchemaKind::Ios6);
        assert!(!schema.supports_smart_albums());
        assert_eq!(schema.relationship_table, "Z_8ASSETS");
        assert_eq!(schema.asset_column, "Z_14ASSETS");
        assert_eq!(schema.order_column, "Z_FOK_14ASSETS");
        // Detection uses capabilities, not a guess based on the reported OS version.
        for version in [6, 12, 26] {
            let (albums, failures) =
                read_albums_from_connection(&connection, version, &schema).unwrap();
            assert_eq!(failures, 0);
            assert_eq!(albums.len(), 2);
            let recents = albums
                .iter()
                .find(|album| album.id == RECENTS_ALBUM_ID)
                .unwrap();
            assert_eq!(recents.item_count, 2);
            assert_eq!(recents.preview_path, "DCIM/100APPLE/two.MOV");
            let custom = albums.iter().find(|album| album.id == 20).unwrap();
            assert_eq!(custom.item_count, 2); // Do not trust stale ZCACHEDCOUNT.
            assert_eq!(custom.preview_path, "DCIM/100APPLE/two.MOV");
            assert!(!albums.iter().any(|album| {
                [FAVS_ALBUM_ID, HIDDEN_ALBUM_ID, RECENTLY_DELETED_ALBUM_ID].contains(&album.id)
            }));
        }
        for recent_first in [true, false] {
            let expected = if recent_first {
                vec!["two.MOV", "one.JPG"]
            } else {
                vec!["one.JPG", "two.MOV"]
            };
            assert_eq!(
                query_names(&connection, &schema.recents_query(recent_first)),
                expected
            );
            for id in [10, 20] {
                let mut statement = connection
                    .prepare(&schema.album_contents_query(recent_first))
                    .unwrap();
                let names = statement
                    .query_map([id], |row| row.get::<_, String>(1))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap();
                assert_eq!(names, expected);
                let image_count = names
                    .iter()
                    .filter(|name| matches_media_filter(name, GalleryMediaFilter::Images))
                    .count();
                let video_count = names
                    .iter()
                    .filter(|name| matches_media_filter(name, GalleryMediaFilter::Videos))
                    .count();
                assert_eq!((image_count, video_count), (1, 1));
            }
        }
    }

    #[test]
    fn ios6_empty_library_and_partial_modern_schemas_are_handled_safely() {
        let connection = ios6_fixture();
        let schema = discover_assets_table(&connection).unwrap();
        connection
            .execute_batch("DELETE FROM ZGENERICASSET;")
            .unwrap();
        let (albums, failures) = read_albums_from_connection(&connection, 6, &schema).unwrap();
        assert_eq!(failures, 0);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].id, RECENTS_ALBUM_ID);
        assert_eq!(albums[0].item_count, 0);
        assert!(query_names(&connection, &schema.recents_query(true)).is_empty());
        connection
            .execute_batch("ALTER TABLE ZGENERICASSET ADD COLUMN ZHIDDEN INTEGER;")
            .unwrap();
        assert!(discover_assets_table(&connection).is_err());
    }

    fn schema_fixture(legacy: bool) -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        let asset_table = if legacy { "ZGENERICASSET" } else { "ZASSET" };
        let asset_column = if legacy { "Z_30ASSETS" } else { "Z_3ASSETS" };
        let order_column = if legacy {
            "Z_FOK_30ASSETS"
        } else {
            "Z_FOK_3ASSETS"
        };
        connection.execute_batch(&format!(
            "CREATE TABLE {asset_table} (
                Z_PK INTEGER PRIMARY KEY, ZDIRECTORY TEXT, ZFILENAME TEXT,
                ZFAVORITE INTEGER, ZTRASHEDSTATE INTEGER, ZVISIBILITYSTATE INTEGER, ZHIDDEN INTEGER
            );
            CREATE TABLE ZGENERICALBUM (
                Z_PK INTEGER PRIMARY KEY, ZTITLE TEXT, ZCACHEDCOUNT INTEGER,
                ZKEYASSET INTEGER, ZCUSTOMKEYASSET INTEGER, ZTRASHEDSTATE INTEGER
            );
            CREATE TABLE Z_23ASSETS (Z_23ALBUMS INTEGER, {asset_column} INTEGER, {order_column} INTEGER);
            CREATE TABLE ZADDITIONALASSETATTRIBUTES (ZORIGINALFILESIZE INTEGER);
            INSERT INTO {asset_table} VALUES
                (1, 'DCIM/100APPLE', 'one.JPG', 0, 0, 0, 0),
                (2, 'DCIM/100APPLE', 'two.MOV', 1, 0, 0, 0),
                (3, 'DCIM/100APPLE', 'hidden.JPG', 0, 0, 0, 1),
                (4, 'DCIM/100APPLE', 'deleted.JPG', 0, 1, 0, 0),
                (5, 'DCIM/100APPLE', 'invisible.JPG', 0, 0, 1, 0);
            INSERT INTO ZGENERICALBUM VALUES (10, 'Test album', 2, 1, 1, 0);
            INSERT INTO Z_23ASSETS VALUES (10, 1, 0), (10, 2, 1), (10, 3, 2), (10, 4, 3);
            INSERT INTO ZADDITIONALASSETATTRIBUTES VALUES (100), (200);"
        )).unwrap();
        connection
    }

    fn query_names(connection: &Connection, query: &str) -> Vec<String> {
        connection
            .prepare(query)
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    #[test]
    fn legacy_and_modern_schemas_support_all_gallery_queries() {
        for (legacy, ios_version, expected_kind) in [
            (true, 12, GallerySchemaKind::Ios12),
            (false, 15, GallerySchemaKind::Modern),
            (false, 26, GallerySchemaKind::Modern),
        ] {
            let connection = schema_fixture(legacy);
            let schema = discover_assets_table(&connection).unwrap();
            assert_eq!(schema.kind, expected_kind);
            assert_eq!(
                schema.asset_column,
                if legacy { "Z_30ASSETS" } else { "Z_3ASSETS" }
            );
            let (albums, failures) =
                read_albums_from_connection(&connection, ios_version, &schema).unwrap();
            assert_eq!(failures, 0);
            for (id, count) in [
                (RECENTS_ALBUM_ID, 2),
                (FAVS_ALBUM_ID, 1),
                (HIDDEN_ALBUM_ID, 2),
                (RECENTLY_DELETED_ALBUM_ID, 1),
                (10, 2),
            ] {
                assert_eq!(
                    albums
                        .iter()
                        .find(|album| album.id == id)
                        .unwrap()
                        .item_count,
                    count
                );
            }
            assert_eq!(
                albums
                    .iter()
                    .find(|album| album.id == 10)
                    .unwrap()
                    .preview_path,
                "DCIM/100APPLE/one.JPG"
            );

            for (template, expected) in [
                (RECENTS_QUERY, vec!["two.MOV", "one.JPG"]),
                (FAVS_QUERY, vec!["two.MOV"]),
                (HIDDEN_QUERY, vec!["invisible.JPG", "hidden.JPG"]),
                (RECENTLY_DELETED_QUERY, vec!["deleted.JPG"]),
            ] {
                assert_eq!(
                    query_names(
                        &connection,
                        &schema.query(&sqlite_ordered_query(template, true))
                    ),
                    expected
                );
                let reverse: Vec<_> = expected.into_iter().rev().collect();
                assert_eq!(
                    query_names(
                        &connection,
                        &schema.query(&sqlite_ordered_query(template, false))
                    ),
                    reverse
                );
            }
            for recent_first in [true, false] {
                let query = schema.query(&format!(
                    "{} ORDER BY ZASSET.Z_PK {}",
                    ALBUM_CONTENTS_QUERY_TEMPLATE,
                    sqlite_order_direction(recent_first)
                ));
                let members = connection
                    .prepare(&query)
                    .unwrap()
                    .query_map([10], |row| row.get::<_, String>(1))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap();
                assert_eq!(
                    members,
                    if recent_first {
                        vec!["two.MOV", "one.JPG"]
                    } else {
                        vec!["one.JPG", "two.MOV"]
                    }
                );
            }
            let size: i64 = connection
                .query_row(GALLERY_TOTAL_SIZE_QUERY, [], |row| row.get(0))
                .unwrap();
            assert_eq!(size, 300);
        }
    }

    #[test]
    fn album_query_selection_preserves_schema_and_version_precedence() {
        for kind in [
            GallerySchemaKind::Ios6,
            GallerySchemaKind::Ios12,
            GallerySchemaKind::Modern,
        ] {
            let connection = match kind {
                GallerySchemaKind::Ios6 => ios6_fixture(),
                GallerySchemaKind::Ios12 => schema_fixture(true),
                GallerySchemaKind::Modern => schema_fixture(false),
            };
            let schema = discover_assets_table(&connection).unwrap();
            for version in [6, 12, 15, 16, 26] {
                let expected = match kind {
                    GallerySchemaKind::Ios6 => schema.query(IOS_6_ALBUM_QUERY_STATEMENT),
                    GallerySchemaKind::Ios12 => schema.query(IOS_12_ALBUM_QUERY_STATEMENT),
                    GallerySchemaKind::Modern if version <= 15 => {
                        IOS_15_ALBUM_QUERY_STATEMENT.to_string()
                    }
                    GallerySchemaKind::Modern => schema.query(IOS_26_ALBUM_QUERY_STATEMENT),
                };
                assert_eq!(
                    album_query(version, &schema),
                    expected,
                    "schema={kind:?}, ios={version}"
                );
            }
        }
    }

    #[test]
    fn legacy_schema_selection_does_not_depend_on_reported_ios_version() {
        let connection = schema_fixture(true);
        let schema = discover_assets_table(&connection).unwrap();
        assert_eq!(
            album_query(26, &schema),
            schema.query(IOS_12_ALBUM_QUERY_STATEMENT)
        );
    }

    #[test]
    fn legacy_empty_library_and_album_without_key_asset_are_supported() {
        let connection = schema_fixture(true);
        connection
            .execute_batch("UPDATE ZGENERICALBUM SET ZKEYASSET=NULL;")
            .unwrap();
        let schema = discover_assets_table(&connection).unwrap();
        let (albums, failures) = read_albums_from_connection(&connection, 12, &schema).unwrap();
        assert_eq!(failures, 0);
        assert!(albums.iter().any(|album| album.id == 10));
        assert_eq!(
            albums
                .iter()
                .find(|album| album.id == 10)
                .unwrap()
                .preview_path,
            "DCIM/100APPLE/two.MOV"
        );
        connection
            .execute_batch(
                "DELETE FROM ZGENERICASSET;
            DELETE FROM Z_23ASSETS; UPDATE ZGENERICALBUM SET ZCACHEDCOUNT=0;",
            )
            .unwrap();
        let (albums, failures) = read_albums_from_connection(&connection, 12, &schema).unwrap();
        assert_eq!(failures, 0);
        assert_eq!(albums.len(), 4);
        assert!(albums.iter().all(|album| album.item_count == 0));
    }

    #[test]
    fn relationship_columns_are_validated() {
        let connection = schema_fixture(true);
        connection
            .execute_batch("ALTER TABLE Z_23ASSETS RENAME COLUMN Z_FOK_30ASSETS TO INVALID;")
            .unwrap();
        assert!(discover_assets_table(&connection).is_err());
    }

    #[test]
    #[ignore = "requires an explicitly supplied local Photos.sqlite snapshot"]
    fn supplied_snapshot_opens_read_only_and_supports_gallery_queries() {
        let path =
            std::env::var("IDESCRIPTOR_TEST_PHOTOS_DB").expect("set IDESCRIPTOR_TEST_PHOTOS_DB");
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let schema = discover_assets_table(&connection).unwrap();
        let (albums, failures) = read_albums_from_connection(&connection, 12, &schema).unwrap();
        assert_eq!(failures, 0);
        let recents = query_names(&connection, &schema.recents_query(true));
        if let Ok(expected) = std::env::var("IDESCRIPTOR_TEST_PHOTOS_EXPECTED_COUNT") {
            assert_eq!(recents.len(), expected.parse::<usize>().unwrap());
        }
        eprintln!(
            "Validated {:?} snapshot: {} gallery media files",
            schema.kind,
            recents.len()
        );
        assert_eq!(
            recents.len(),
            albums
                .iter()
                .find(|album| album.id == RECENTS_ALBUM_ID)
                .unwrap()
                .item_count as usize
        );
        if schema.supports_smart_albums() {
            for template in [FAVS_QUERY, HIDDEN_QUERY, RECENTLY_DELETED_QUERY] {
                query_names(&connection, &schema.query(template));
            }
        } else {
            assert_eq!(schema.kind, GallerySchemaKind::Ios6);
            assert!(!albums.iter().any(|album| {
                [FAVS_ALBUM_ID, HIDDEN_ALBUM_ID, RECENTLY_DELETED_ALBUM_ID].contains(&album.id)
            }));
        }
        for album in &albums {
            if album.id >= 0 {
                let query = schema.album_contents_query(true);
                let count = connection
                    .prepare(&query)
                    .unwrap()
                    .query_map([album.id], |_| Ok(()))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap()
                    .len();
                assert_eq!(count, album.item_count as usize);
            }
        }
        let size: i64 = connection
            .query_row(GALLERY_TOTAL_SIZE_QUERY, [], |row| row.get(0))
            .unwrap();
        assert!(size >= 0);
    }

    fn metadata(size: usize, modified: &str) -> RemoteFileMetadata {
        RemoteFileMetadata {
            size,
            modified: modified.to_string(),
        }
    }

    #[test]
    fn snapshot_change_detection_uses_presence_size_and_modified_time() {
        let mut committed = SnapshotMetadata::new();
        committed.insert(SnapshotFile::Database, Some(metadata(100, "1")));
        committed.insert(SnapshotFile::Wal, None);
        committed.insert(SnapshotFile::Shm, Some(metadata(50, "2")));

        let unchanged = committed.clone();
        assert!(!snapshot_file_changed(
            &committed,
            &unchanged,
            SnapshotFile::Database
        ));

        let mut size_changed = committed.clone();
        size_changed.insert(SnapshotFile::Database, Some(metadata(101, "1")));
        assert!(snapshot_file_changed(
            &committed,
            &size_changed,
            SnapshotFile::Database
        ));

        let mut modified_changed = committed.clone();
        modified_changed.insert(SnapshotFile::Shm, Some(metadata(50, "3")));
        assert!(snapshot_file_changed(
            &committed,
            &modified_changed,
            SnapshotFile::Shm
        ));

        let mut appeared = committed.clone();
        appeared.insert(SnapshotFile::Wal, Some(metadata(10, "4")));
        assert!(snapshot_file_changed(
            &committed,
            &appeared,
            SnapshotFile::Wal
        ));

        let mut disappeared = committed.clone();
        disappeared.insert(SnapshotFile::Shm, None);
        assert!(snapshot_file_changed(
            &committed,
            &disappeared,
            SnapshotFile::Shm
        ));
    }

    #[test]
    fn invalid_database_fails_quick_check() {
        let path = std::env::temp_dir().join(format!(
            "idescriptor-invalid-gallery-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, b"not a sqlite database").unwrap();

        assert!(open_and_validate_connection(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn incomplete_database_fails_album_query_validation() {
        let path = std::env::temp_dir().join(format!(
            "idescriptor-incomplete-gallery-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let connection = Connection::open(&path).unwrap();
        connection
            .execute("CREATE TABLE Z_1ASSETS (Z_PK INTEGER)", [])
            .unwrap();
        connection.close().unwrap();

        assert!(open_and_validate_connection(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn modern_album_query_replaces_dynamic_schema_placeholders() {
        let schema = GallerySchema {
            kind: GallerySchemaKind::Modern,
            asset_table: "ZASSET",
            relationship_table: "Z_33ASSETS".into(),
            album_column: "Z_33ALBUMS".into(),
            asset_column: "Z_3ASSETS".into(),
            order_column: "Z_FOK_3ASSETS".into(),
        };
        let query = album_query(26, &schema);

        assert!(!query.contains("{table}"));
        assert!(!query.contains("{album}"));
        assert!(query.contains("Z_33ASSETS.Z_3ASSETS"));
        assert!(query.contains("Z_33ASSETS.Z_33ALBUMS"));
    }

    #[test]
    fn built_in_album_content_queries_have_valid_clause_order() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute(
                "CREATE TABLE ZASSET (
                    Z_PK INTEGER,
                    ZFILENAME TEXT,
                    ZDIRECTORY TEXT,
                    ZFAVORITE INTEGER,
                    ZTRASHEDSTATE INTEGER,
                    ZVISIBILITYSTATE INTEGER,
                    ZHIDDEN INTEGER
                )",
                [],
            )
            .unwrap();

        for query in [
            RECENTS_QUERY,
            FAVS_QUERY,
            RECENTLY_DELETED_QUERY,
            HIDDEN_QUERY,
        ] {
            connection
                .prepare(&sqlite_ordered_query(query, true))
                .unwrap();
            connection
                .prepare(&sqlite_ordered_query(query, false))
                .unwrap();
        }

        connection.prepare(HIDDEN_ALBUM_QUERY).unwrap();
    }
}
