// ============================================================================
// Local adjustor thunks for two symbols that Qt 6.9.3's OFFICIAL mingw
// Qt6Quick.dll forgot to export but its own GraphicalEffects private plugin
// imports (Qt packaging defect; see windows-env/scripts/
// build-qtgraphicaleffects.sh for the full story).
//
// These are byte-exact replacements of the missing Itanium-ABI non-virtual
// thunks: receive `this` = QQmlParserStatus subobject pointer, adjust
// -0x10 to reach the QQuickItem base, then tail-jump to the real method
// (which Qt6Quick.dll DOES export). Implemented in asm so semantics cannot
// drift from the real thunks.
//
// Defining them inside the plugin satisfies its own imports at link time;
// the phantom imports disappear from the built DLL.
// ============================================================================

#ifdef __MINGW32__
asm(
    ".text\n"
    ".globl _ZThn16_N10QQuickItem10classBeginEv\n"
    "_ZThn16_N10QQuickItem10classBeginEv:\n"
    "   subq $16, %rcx\n"
    "   jmp _ZN10QQuickItem10classBeginEv\n"

    ".globl _ZThn16_N10QQuickItem17componentCompleteEv\n"
    "_ZThn16_N10QQuickItem17componentCompleteEv:\n"
    "   subq $16, %rcx\n"
    "   jmp _ZN10QQuickItem17componentCompleteEv\n"
);
#else
#error "This shim is only meant for the MinGW64 build of qtgraphicaleffectsprivate"
#endif
