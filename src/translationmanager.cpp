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

#include "translationmanager.h"

#include "settingsmanager.h"

#include <QCoreApplication>
#include <QDebug>
#include <QLibraryInfo>
#include <QTranslator>

TranslationManager *TranslationManager::sharedInstance()
{
    static TranslationManager instance;
    return &instance;
}

TranslationManager::TranslationManager(QObject *parent) : QObject{parent}
{
}

QStringList TranslationManager::availableLocaleCodes()
{
    return QStringList{QStringLiteral(""), QStringLiteral("en"),
                       QStringLiteral("ja"), QStringLiteral("zh_CN")};
}

QString TranslationManager::displayName(const QString &code)
{
    if (code.isEmpty()) {
        return tr("System");
    }
    if (code == QStringLiteral("en")) {
        return QStringLiteral("English");
    }
    if (code == QStringLiteral("ja")) {
        return QStringLiteral("日本語");
    }
    if (code == QStringLiteral("zh_CN")) {
        return QStringLiteral("简体中文");
    }
    return code;
}

void TranslationManager::initFromSettings()
{
    const QString persisted = SettingsManager::sharedInstance()->language();
    m_currentCode = persisted;
    applyLocale(persisted);
}

void TranslationManager::setLocale(const QString &code)
{
    if (code == m_currentCode && (m_appTranslator || m_qtTranslator)) {
        return;
    }

    applyLocale(code);
    m_currentCode = code;
    SettingsManager::sharedInstance()->setLanguage(code);
    emit localeChanged(code);
}

QString TranslationManager::currentLocaleCode() const
{
    return m_currentCode;
}

QLocale TranslationManager::resolveLocale(const QString &code) const
{
    if (code.isEmpty()) {
        return QLocale::system();
    }
    return QLocale(code);
}

void TranslationManager::applyLocale(const QString &code)
{
    if (m_appTranslator) {
        QCoreApplication::removeTranslator(m_appTranslator);
        m_appTranslator->deleteLater();
        m_appTranslator = nullptr;
    }
    if (m_qtTranslator) {
        QCoreApplication::removeTranslator(m_qtTranslator);
        m_qtTranslator->deleteLater();
        m_qtTranslator = nullptr;
    }

    const QLocale locale = resolveLocale(code);

    m_appTranslator = new QTranslator(this);
    if (!m_appTranslator->load(locale, QStringLiteral("idescriptor"),
                               QStringLiteral("_"),
                               QStringLiteral(":/i18n"))) {
        qWarning() << "TranslationManager: failed to load app translation for"
                   << locale.name();
    }
    QCoreApplication::installTranslator(m_appTranslator);

    m_qtTranslator = new QTranslator(this);
    if (!m_qtTranslator->load(
            locale, QStringLiteral("qt"), QStringLiteral("_"),
            QLibraryInfo::path(QLibraryInfo::TranslationsPath))) {
        qWarning() << "TranslationManager: failed to load Qt translation for"
                   << locale.name();
    }
    QCoreApplication::installTranslator(m_qtTranslator);
}
