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

#ifndef IDESCRIPTOR_I18N_HELPER_H
#define IDESCRIPTOR_I18N_HELPER_H

#include <QCoreApplication>
#include <QString>

// Thin wrappers around QCoreApplication::translate() so that translation
// lookups can be performed by code that does not have access to a QObject
// subclass (notably future Rust modules linked through cxx-qt). The
// implementation simply forwards to Qt's translation infrastructure, which
// keeps the .ts/.qm pipeline as the single source of truth.

namespace idescriptor::i18n
{

inline QString tr(const char *context, const char *sourceText)
{
    return QCoreApplication::translate(context, sourceText);
}

inline QString tr(const char *context, const char *sourceText,
                  const char *disambiguation, int n = -1)
{
    return QCoreApplication::translate(context, sourceText, disambiguation, n);
}

} // namespace idescriptor::i18n

// Future C ABI wrappers for cxx-qt / Rust integration are intentionally
// omitted in this initial PR; the namespace above is sufficient for C++
// callers and provides a stable seam to extend.

#endif // IDESCRIPTOR_I18N_HELPER_H
