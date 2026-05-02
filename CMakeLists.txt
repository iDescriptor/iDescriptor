cmake_minimum_required(VERSION 3.16)
project(iDescriptor VERSION 0.1.0 LANGUAGES CXX)

# Feature options
option(ENABLE_RECOVERY_DEVICE_SUPPORT "Enable recovery device support (requires libirecovery)" ON)
set(PACKAGE_MANAGER_HINT "" CACHE STRING "Name of package manager(s) used to manage this build (e.g. paru, yay, pamac)")
option(PACKAGE_MANAGER_MANAGED "Build as package manager managed version (auto updates will be handled by the package manager)" OFF)
option(DEPLOY "Deploy the application (WIN32 only)" ON)

set(CMAKE_AUTOUIC ON)
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

if (APPLE) 
    # Target at least macOS 14.0
    set(CMAKE_OSX_DEPLOYMENT_TARGET "14.0")
endif()

# Platform-specific paths for libraries built from source
if(WIN32)
    include_directories("C:/msys64/mingw64/include")
    link_directories("C:/msys64/mingw64/lib")
    set(PKG_CONFIG_EXECUTABLE "C:/msys64/mingw64/bin/pkg-config.exe")
    list(APPEND CMAKE_PREFIX_PATH "C:/lxqt")
    set(CUSTOM_LIB_PATH "C:/msys64/mingw64/lib")
    set(CUSTOM_INCLUDE_PATH "C:/msys64/mingw64/include")
    set(CUSTOM_PKGCONFIG_PATH "C:/msys64/mingw64/lib/pkgconfig")
    set(ENV{PKG_CONFIG_PATH} "${CUSTOM_PKGCONFIG_PATH};$ENV{PKG_CONFIG_PATH}")
elseif(APPLE)
    # Get Homebrew prefix
    execute_process(
        COMMAND brew --prefix
        OUTPUT_VARIABLE HOMEBREW_PREFIX
        OUTPUT_STRIP_TRAILING_WHITESPACE
    )
    set(CUSTOM_LIB_PATH "/usr/local/lib")
    set(CUSTOM_INCLUDE_PATH "/usr/local/include")
    set(CUSTOM_PKGCONFIG_PATH "/usr/local/lib/pkgconfig")
    if(HOMEBREW_PREFIX)
        set(CUSTOM_PKGCONFIG_PATH "${HOMEBREW_PREFIX}/lib/pkgconfig:${CUSTOM_PKGCONFIG_PATH}")
        # Add Homebrew paths
        include_directories(${HOMEBREW_PREFIX}/include)
    endif()
    set(ENV{PKG_CONFIG_PATH} "${CUSTOM_PKGCONFIG_PATH}:$ENV{PKG_CONFIG_PATH}")
else ()
    set(CUSTOM_LIB_PATH "/usr/local/lib")
    set(CUSTOM_PKGCONFIG_PATH "/usr/local/lib/pkgconfig")
    set(ENV{PKG_CONFIG_PATH} "$ENV{PKG_CONFIG_PATH}:${CUSTOM_PKGCONFIG_PATH}")
endif()

include_directories(${CUSTOM_INCLUDE_PATH})


foreach(_ IN LISTS CMAKE_PREFIX_PATH)
    list(APPEND _qt_pkg_dirs
        "${_}/lib/pkgconfig"
        "${_}/lib64/pkgconfig"
        "${_}/lib64/qt/pkgconfig"
        "${_}/lib/qt/pkgconfig"
    )
endforeach()


list(APPEND _qt_pkg_dirs ${CUSTOM_PKGCONFIG_PATH})
 
find_package(PkgConfig REQUIRED)
find_package(Qt6 REQUIRED COMPONENTS Widgets Multimedia MultimediaWidgets Network QuickControls2 SerialPort Positioning Location QuickWidgets) 

# Add QTermWidget
# Prefer CMake-native qtermwidget6, fallback to pkg-config if needed
find_package(qtermwidget6 QUIET)

if(NOT qtermwidget6_FOUND)
    message(STATUS "qtermwidget6 not found via CMake, trying pkg-config...")
    find_package(PkgConfig REQUIRED)
    pkg_check_modules(QTERMWIDGET REQUIRED IMPORTED_TARGET qtermwidget6)
    # Create alias so we can use qtermwidget6::qtermwidget6 uniformly
    add_library(qtermwidget6 ALIAS PkgConfig::QTERMWIDGET)
else()
    message(STATUS "Found qtermwidget6 via CMake")
endif()

if(WIN32)
    # Get the path to the Qt bin directory
    get_target_property(QT_BIN_PATH Qt${QT_VERSION_MAJOR}::Core IMPORTED_LOCATION_RELEASE)
    if(NOT QT_BIN_PATH)
        get_target_property(QT_BIN_PATH Qt${QT_VERSION_MAJOR}::Core IMPORTED_LOCATION_DEBUG)
    endif()
    if(NOT QT_BIN_PATH)
        get_target_property(QT_BIN_PATH Qt${QT_VERSION_MAJOR}::Core IMPORTED_LOCATION)
    endif()
    get_filename_component(QT_BIN_PATH ${QT_BIN_PATH} DIRECTORY)
    message(STATUS "Found Qt bin directory: ${QT_BIN_PATH}")
endif()

# Define library search behavior based on platform
if(LINUX)
    # On Linux (AUR builds), let CMake search default system paths first.
    # The custom path /usr/local/lib will be checked if it's in the default search paths.
    set(CUSTOM_FIND_LIB_ARGS "")
elseif(APPLE)
    # On macOS, search both custom path and Homebrew paths
    execute_process(
        COMMAND brew --prefix
        OUTPUT_VARIABLE HOMEBREW_PREFIX
        OUTPUT_STRIP_TRAILING_WHITESPACE
    )
    set(CUSTOM_FIND_LIB_ARGS
        PATHS ${CUSTOM_LIB_PATH} ${HOMEBREW_PREFIX}/lib
    )
else()
    # On other platforms, only search the custom path for our specific libraries.
    set(CUSTOM_FIND_LIB_ARGS
        PATHS ${CUSTOM_LIB_PATH}
        NO_DEFAULT_PATH
    )
endif()

find_library(IMOBILEDEVICE_LIBRARY 
    NAMES imobiledevice-1.0
    ${CUSTOM_FIND_LIB_ARGS}
    REQUIRED
)

find_library(IMOBILEDEVICE_GLUE_LIBRARY 
    NAMES imobiledevice-glue-1.0
    ${CUSTOM_FIND_LIB_ARGS}
    REQUIRED
)

find_library(TATSU_LIBRARY 
    NAMES tatsu
    ${CUSTOM_FIND_LIB_ARGS}
    REQUIRED
)

# Add QR code generation library
pkg_check_modules(QRENCODE REQUIRED IMPORTED_TARGET libqrencode)
pkg_check_modules(HEIF REQUIRED IMPORTED_TARGET libheif)
pkg_check_modules(ZIP REQUIRED IMPORTED_TARGET libzip)

# Add FFmpeg libraries for video thumbnail generation
pkg_check_modules(AVFORMAT REQUIRED IMPORTED_TARGET libavformat)
pkg_check_modules(AVCODEC REQUIRED IMPORTED_TARGET libavcodec)
pkg_check_modules(AVUTIL REQUIRED IMPORTED_TARGET libavutil)
pkg_check_modules(SWSCALE REQUIRED IMPORTED_TARGET libswscale)

if(ENABLE_RECOVERY_DEVICE_SUPPORT)
    find_library(IRECOVERY_LIBRARY 
        NAMES irecovery-1.0
        PATHS ${CUSTOM_LIB_PATH}
        NO_DEFAULT_PATH
    )
    if(IRECOVERY_LIBRARY)
        message(STATUS "Building with recovery device support enabled")
    else()
        message(WARNING "libirecovery not found. Recovery device support will be disabled. This is to be expected if you are installing from Arch AUR.")
        set(ENABLE_RECOVERY_DEVICE_SUPPORT OFF)
    endif()
else()
    message(STATUS "Recovery device support disabled")
endif()

find_library(USBMUXD_LIBRARY 
    NAMES usbmuxd-2.0
    ${CUSTOM_FIND_LIB_ARGS}
    REQUIRED
)

if(WIN32)
    # On MSYS2, these are found in the standard mingw64 prefix
    find_library(SSL_LIBRARY NAMES ssl PATHS C:/msys64/mingw64/lib REQUIRED)
    find_library(CRYPTO_LIBRARY NAMES crypto PATHS C:/msys64/mingw64/lib REQUIRED)
else()
    find_library(SSL_LIBRARY NAMES ssl REQUIRED)
    find_library(CRYPTO_LIBRARY NAMES crypto REQUIRED)
endif()

# Add libssh for SSH connections
pkg_check_modules(SSH REQUIRED IMPORTED_TARGET libssh)

# Apple-specific crypto libraries for SSH
if(APPLE)
    find_library(SECURITY_FRAMEWORK Security REQUIRED)
    find_library(COREFOUNDATION_FRAMEWORK CoreFoundation REQUIRED)
endif()


pkg_check_modules(PUGIXML REQUIRED IMPORTED_TARGET pugixml)
pkg_check_modules(USB REQUIRED IMPORTED_TARGET libusb-1.0)
pkg_check_modules(PLIST REQUIRED IMPORTED_TARGET libplist-2.0)


file(GLOB PROJECT_SOURCES
src/*.cpp
src/core/helpers/*.cpp
src/core/services/*.cpp
src/*.h
src/*.ui
resources.qrc
)

if(APPLE)
    list(APPEND PROJECT_SOURCES
        src/platform/macos.mm
        src/core/services/dnssd/dnssd_service.cpp
        src/core/services/dnssd/dnssd_service.h
    )
endif()

if (WIN32)
    list(APPEND PROJECT_SOURCES
       src/core/services/dnssd/dnssd_service.cpp
        src/core/services/dnssd/dnssd_service.h
    )

    file(GLOB WINDOWS_PLATFORM_SOURCES src/platform/windows/*.cpp src/platform/windows/*.h)
    list(APPEND PROJECT_SOURCES ${WINDOWS_PLATFORM_SOURCES})
endif()

if(LINUX)
    list(APPEND PROJECT_SOURCES
        src/core/services/avahi/avahi_service.cpp
        src/core/services/avahi/avahi_service.h
    )
endif()

if (NOT ENABLE_RECOVERY_DEVICE_SUPPORT)
    list(REMOVE_ITEM PROJECT_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/src/recoverydeviceinfowidget.cpp
        ${CMAKE_CURRENT_SOURCE_DIR}/src/recoverydeviceinfowidget.h
        ${CMAKE_CURRENT_SOURCE_DIR}/src/core/helpers/parse_recovery_mode.cpp
        src/recoverydeviceinfowidget.cpp
        src/recoverydeviceinfowidget.h
        src/core/helpers/parse_recovery_mode.cpp
    )
endif()

add_subdirectory(lib/airplay)
add_subdirectory(lib/ipatool-go)
add_subdirectory(lib/zupdater)

if (WIN32) 
    set(NO_DEPLOY_WIN_IFUSE ON) 
    add_subdirectory(lib/win-ifuse)
endif()



if (WIN32)
    set(app_icon_resource_windows "${CMAKE_CURRENT_SOURCE_DIR}/idescriptor.rc")
    qt_add_executable(iDescriptor
    MANUAL_FINALIZATION
    ${PROJECT_SOURCES}
    ${app_icon_resource_windows}
)
elseif (APPLE)
    set(MACOSX_BUNDLE_ICON_FILE icon.icns)
    set(app_icon_macos "${CMAKE_CURRENT_SOURCE_DIR}/resources/icons/app-icon/icon.icns")
    set_source_files_properties(${app_icon_macos} PROPERTIES
           MACOSX_PACKAGE_LOCATION "Resources")

    qt_add_executable(iDescriptor
    MANUAL_FINALIZATION
    ${PROJECT_SOURCES}
    ${app_icon_macos}
)
else()
    qt_add_executable(iDescriptor
    MANUAL_FINALIZATION
    ${PROJECT_SOURCES}
)
endif()

target_link_libraries(iDescriptor PRIVATE 
    Qt6::Widgets 
    Qt6::Multimedia
    Qt6::MultimediaWidgets
    Qt6::Network
    Qt6::Core
    Qt6::Quick
    Qt6::Location
    Qt6::Positioning
    Qt6::QuickWidgets
    Qt6::QuickControls2
    ${IMOBILEDEVICE_LIBRARY}
    ${IMOBILEDEVICE_GLUE_LIBRARY}
    ${TATSU_LIBRARY}
    ${SSL_LIBRARY}
    ${CRYPTO_LIBRARY}
    PkgConfig::SSH
    ${SSH_LIBRARY}
    ${USBMUXD_LIBRARY}
    PkgConfig::PUGIXML
    PkgConfig::USB
    PkgConfig::PLIST
    PkgConfig::QRENCODE
    qtermwidget6
    PkgConfig::HEIF
    PkgConfig::ZIP
    PkgConfig::AVFORMAT
    PkgConfig::AVCODEC
    PkgConfig::AVUTIL
    PkgConfig::SWSCALE
    airplay
    ipatool-go
    ZUpdater
)

# Conditionally link libirecovery
if(ENABLE_RECOVERY_DEVICE_SUPPORT)
    target_link_libraries(iDescriptor PRIVATE ${IRECOVERY_LIBRARY})
endif()

target_include_directories(iDescriptor PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/lib/zupdater/src
)

if(APPLE)
    find_library(CORE_SERVICES_FRAMEWORK CoreServices REQUIRED)
    target_link_libraries(iDescriptor PRIVATE
    ${CORE_SERVICES_FRAMEWORK})
    message(STATUS "Using macOS Bonjour framework for network service discovery")
elseif (WIN32)
    find_path(DNSSD_INCLUDE_DIR dns_sd.h HINTS ${BONJOUR_SDK}/Include ) 
    target_include_directories( iDescriptor PRIVATE ${DNSSD_INCLUDE_DIR} )
    message( STATUS "Using Bonjour SDK for network service discovery" )
else()
    pkg_check_modules(AVAHI_CLIENT REQUIRED IMPORTED_TARGET avahi-client)

    target_link_libraries(iDescriptor PRIVATE
    PkgConfig::AVAHI_CLIENT
    # PkgConfig::AVAHI_COMMON
    )
    message(STATUS "Using Avahi for network service discovery")
endif()


# Add Apple-specific frameworks for SSH
if(APPLE)
    target_link_libraries(iDescriptor PRIVATE
        ${SECURITY_FRAMEWORK}
        ${COREFOUNDATION_FRAMEWORK}
    )
endif()

# Add compile definition for source directory
target_compile_definitions(iDescriptor PRIVATE 
    SOURCE_DIR="${CMAKE_SOURCE_DIR}"
)

# Add compile definition for recovery device support
if(ENABLE_RECOVERY_DEVICE_SUPPORT)
    target_compile_definitions(iDescriptor PRIVATE ENABLE_RECOVERY_DEVICE_SUPPORT)
endif()

if(PACKAGE_MANAGER_MANAGED)
    target_compile_definitions(iDescriptor PRIVATE PACKAGE_MANAGER_MANAGED)
    message(STATUS "Building as package manager managed version, updates will be handled by the package manager")
    if(PACKAGE_MANAGER_HINT)
        message(STATUS "Configured package manager hint: ${PACKAGE_MANAGER_HINT}")
        target_compile_definitions(iDescriptor PRIVATE PACKAGE_MANAGER_HINT=\"${PACKAGE_MANAGER_HINT}\")
    else()
        target_compile_definitions(iDescriptor PRIVATE PACKAGE_MANAGER_HINT="your package manager")
    endif()
endif()


target_compile_definitions(iDescriptor PRIVATE 
    APP_VERSION="${PROJECT_VERSION}"
)

set_target_properties(iDescriptor PROPERTIES
    ${BUNDLE_ID_OPTION}
    MACOSX_BUNDLE_BUNDLE_VERSION ${PROJECT_VERSION}
    MACOSX_BUNDLE_SHORT_VERSION_STRING ${PROJECT_VERSION_MAJOR}.${PROJECT_VERSION_MINOR}
    MACOSX_BUNDLE TRUE
    WIN32_EXECUTABLE TRUE
    BUILD_WITH_INSTALL_RPATH TRUE
)
if (UNIX AND NOT APPLE)
    # Required on Linux to find libirecovery-1.0.so.5 at runtime
    if (ENABLE_RECOVERY_DEVICE_SUPPORT)
    set_target_properties(iDescriptor PROPERTIES
        # Control library search order - system libs first, then /usr/local/lib
        INSTALL_RPATH "/usr/lib/x86_64-linux-gnu:/usr/lib:/usr/local/lib:$ORIGIN"
    )
    endif()
# Add install rules for the project
include(GNUInstallDirs)

# Install the main executable
install(TARGETS iDescriptor
    RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR}
)

# Install the .desktop file
install(FILES ${CMAKE_CURRENT_SOURCE_DIR}/iDescriptor.desktop
    DESTINATION ${CMAKE_INSTALL_DATADIR}/applications
)

# Install the application icon
install(FILES ${CMAKE_CURRENT_SOURCE_DIR}/resources/icons/app-icon/icon.png
    RENAME iDescriptor.png
    DESTINATION ${CMAKE_INSTALL_DATADIR}/icons/hicolor/256x256/apps
)
endif()

include(GNUInstallDirs)

if (WIN32)
# Set the installation directory to be within the build folder
set(CMAKE_INSTALL_PREFIX "${CMAKE_BINARY_DIR}/dist")
endif()
if(QT_VERSION_MAJOR EQUAL 6)
    qt_finalize_executable(iDescriptor)
endif()

# Copy runtime DLLs to build directory after building
if(WIN32 AND DEPLOY)
    add_custom_command(TARGET iDescriptor POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E echo "Starting Windows deployment..."
        COMMAND ${CMAKE_COMMAND}
            -DEXECUTABLE_PATH=$<TARGET_FILE:iDescriptor>
            -DQT_BIN_PATH=${QT_BIN_PATH}
            -DMSYS2_BIN_PATH=C:/msys64/mingw64/bin
            -DOUTPUT_DIR=$<TARGET_FILE_DIR:iDescriptor>
            -DQML_SOURCE_DIR=${CMAKE_CURRENT_SOURCE_DIR}/qml
            -DPROJECT_SOURCE_DIR=${CMAKE_SOURCE_DIR}
            -DWIN_IFUSE=$<TARGET_FILE:win-ifuse>
            -P ${CMAKE_CURRENT_LIST_DIR}/cmake/win-deploy.cmake
        COMMENT "Deploying Windows application with all dependencies"
        VERBATIM
    )
endif()

if(WIN32)
    install(CODE "
        message(STATUS \"Deploying dependencies to installation directory for packaging...\")
        message(STATUS \"CMAKE_INSTALL_PREFIX: \${CMAKE_INSTALL_PREFIX}\")
        # copy executable to dist dir
        file(MAKE_DIRECTORY \"\${CMAKE_INSTALL_PREFIX}\")
        # file(COPY <TARGET_FILE:iDescriptor> DESTINATION CMAKE_INSTALL_PREFIX)
        file(COPY ${CMAKE_CURRENT_BINARY_DIR}/iDescriptor.exe DESTINATION ${CMAKE_INSTALL_PREFIX})
        # Check if file exists before deployment
        set(EXECUTABLE_PATH \"\${CMAKE_INSTALL_PREFIX}/iDescriptor.exe\")
        message(STATUS \"Looking for executable at: \${EXECUTABLE_PATH}\")
        
        if(EXISTS \"\${EXECUTABLE_PATH}\")
            message(STATUS \"SUCCESS: Executable found at \${EXECUTABLE_PATH}\")
        else()
            message(STATUS \"ERROR: Executable NOT found at \${EXECUTABLE_PATH}\")
        endif()
        
        execute_process(
            COMMAND \"${CMAKE_COMMAND}\"
                -DEXECUTABLE_PATH=\${EXECUTABLE_PATH}
                -DQT_BIN_PATH=\"${QT_BIN_PATH}\"
                -DMSYS2_BIN_PATH=\"C:/msys64/mingw64/bin\"
                -DOUTPUT_DIR=\"\${CMAKE_INSTALL_PREFIX}\"
                -DQML_SOURCE_DIR=\"${CMAKE_CURRENT_SOURCE_DIR}/qml\"
                -DPROJECT_SOURCE_DIR=\"${CMAKE_SOURCE_DIR}\"
                -DWIN_IFUSE=$<TARGET_FILE:win-ifuse>
                -P \"${CMAKE_CURRENT_LIST_DIR}/cmake/win-deploy.cmake\"
        )
    ")
endif()

# Packaging Configuration (CPack)
set(CPACK_PACKAGE_NAME ${PROJECT_NAME})
set(CPACK_PACKAGE_VERSION ${PROJECT_VERSION})
set(CPACK_WIX_VERSION 4)
set(CPACK_PACKAGE_DESCRIPTION_SUMMARY "iDescriptor Application")
set(CPACK_PACKAGE_VENDOR "iDescriptor")
set(CPACK_PACKAGE_CONTACT "https://github.com/iDescriptor/iDescriptor")

set(CPACK_OUTPUT_FILE_PREFIX "${CMAKE_BINARY_DIR}/artifacts")

if(WIN32)
    set(CPACK_GENERATOR "WIX;ZIP")
    # FIXME: arm64 build support
    set(CPACK_PACKAGE_FILE_NAME "${PROJECT_NAME}-v${PROJECT_VERSION}-Windows_x86_64")

    set(CPACK_PACKAGE_INSTALL_DIRECTORY "${PROJECT_NAME}")

    string(UUID CPACK_WIX_PRODUCT_GUID NAMESPACE "1a2b3c4d-5e6f-7a8b-9c0d-1e2f3a4b5c6d" NAME "${PROJECT_NAME}-${PROJECT_VERSION}" TYPE MD5)
    string(UUID CPACK_WIX_UPGRADE_GUID NAMESPACE "d6c5b4a3-f2e1-d0c9-b8a7-f6e5d4c3b2a1" NAME "${PROJECT_NAME}" TYPE MD5)
    set(CPACK_WIX_UI_REF "WixUI_InstallDir")
    
    set(CPACK_WIX_UI_BANNER "${CMAKE_CURRENT_SOURCE_DIR}/resources/installer/banner.bmp")
    set(CPACK_WIX_UI_DIALOG "${CMAKE_CURRENT_SOURCE_DIR}/resources/installer/dialog.bmp")

    set(CPACK_WIX_PROPERTY_ARPCOMMENTS "A free, open-source, and cross-platform iDevice management tool written in C++.")
    set(CPACK_WIX_PROPERTY_ARPHELPLINK "${CPACK_PACKAGE_CONTACT}/issues")
    set(CPACK_WIX_PROPERTY_ARPURLINFOABOUT "${CPACK_PACKAGE_CONTACT}")
    set(CPACK_WIX_PROPERTY_ARPURLUPDATEINFO "${CPACK_PACKAGE_CONTACT}/releases")

    set(CPACK_WIX_INSTALL_SCOPE "perMachine")
    set(CPACK_WIX_PROGRAM_MENU_FOLDER "${PROJECT_NAME}")
    set(CPACK_PACKAGE_EXECUTABLES "iDescriptor" "iDescriptor")
    set(CPACK_CREATE_DESKTOP_LINKS "iDescriptor")
    set(CPACK_WIX_PRODUCT_ICON "${CMAKE_CURRENT_SOURCE_DIR}/resources/icons/app-icon/icon.ico")
    set(CPACK_WIX_LICENSE_RTF "${CMAKE_CURRENT_SOURCE_DIR}/resources/installer/LICENSE.rtf")

    set(CPACK_INCLUDE_TOPLEVEL_DIRECTORY OFF)
    set(CPACK_ARCHIVE_COMPONENT_INSTALL TRUE)
    set(CPACK_ARCHIVE_INSTALL_DIRECTORY ".")
    
    # Tell CPack to use the pre-built dist directory
    set(CPACK_INSTALLED_DIRECTORIES "${CMAKE_INSTALL_PREFIX};.")
    
    # Prevent CPack from running install again
    set(CPACK_INSTALL_CMAKE_PROJECTS "")
endif()

include(CPack)

