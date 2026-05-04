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

#ifndef TRANSLATIONMANAGER_H
#define TRANSLATIONMANAGER_H

#include <QLocale>
#include <QObject>
#include <QString>
#include <QStringList>

class QTranslator;

class TranslationManager : public QObject
{
    Q_OBJECT

public:
    static TranslationManager *sharedInstance();

    // Locale codes bundled with the app. The empty string acts as a sentinel
    // meaning "follow the host system locale".
    static QStringList availableLocaleCodes();

    // Human-readable label for a code, suitable for a Settings combo box.
    // Empty string maps to tr("System"); known codes map to their endonyms.
    static QString displayName(const QString &code);

    // Initialise translators from persisted settings (or system locale).
    // Must be called from main() right after QApplication construction and
    // QCoreApplication::setOrganizationName/setApplicationName.
    void initFromSettings();

    // Apply a locale immediately and persist the choice. An empty string
    // resets the override to the system locale.
    void setLocale(const QString &code);

    // Persisted override ("" if following the system locale).
    QString currentLocaleCode() const;

signals:
    void localeChanged(const QString &code);

private:
    explicit TranslationManager(QObject *parent = nullptr);

    // Helper: resolves "" / specific code into the QLocale we should load.
    QLocale resolveLocale(const QString &code) const;

    // Internal: install translators for the given code (does not persist).
    void applyLocale(const QString &code);

    QTranslator *m_appTranslator = nullptr;
    QTranslator *m_qtTranslator = nullptr;
    QString m_currentCode;
};

#endif // TRANSLATIONMANAGER_H
