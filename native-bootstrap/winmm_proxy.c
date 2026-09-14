#define WIN32_LEAN_AND_MEAN
#define _WIN32_WINNT 0x0601
#include <windows.h>
#include <string.h>
#include <wchar.h>

typedef UINT MMRESULT;
typedef MMRESULT(WINAPI *period_fn)(UINT);
typedef void (*py_initialize_fn)(void);
typedef int (*py_run_simple_string_fn)(const char *);
typedef int (*py_finalize_ex_fn)(void);

#define TIMERR_NOCANDO 97U
#define LOAD_SYMBOL(target, module, name)       \
    do {                                        \
        FARPROC symbol = GetProcAddress(module, name); \
        memcpy(&(target), &symbol, sizeof(target));     \
    } while (0)

static HMODULE self_module;
static HMODULE real_winmm;
static INIT_ONCE real_winmm_once = INIT_ONCE_STATIC_INIT;
static period_fn real_time_begin_period;
static period_fn real_time_end_period;

static BOOL CALLBACK load_real_winmm(PINIT_ONCE once, PVOID parameter, PVOID *context) {
    (void)once;
    (void)parameter;
    (void)context;
    wchar_t path[MAX_PATH];
    UINT length = GetSystemDirectoryW(path, MAX_PATH);
    if (!length || length >= MAX_PATH - 11) {
        return FALSE;
    }
    wcscat(path, L"\\winmm.dll");
    real_winmm = LoadLibraryW(path);
    if (!real_winmm) {
        return FALSE;
    }
    LOAD_SYMBOL(real_time_begin_period, real_winmm, "timeBeginPeriod");
    LOAD_SYMBOL(real_time_end_period, real_winmm, "timeEndPeriod");
    return real_time_begin_period && real_time_end_period;
}

static void append_boot_log(const wchar_t *runtime_root, const char *message) {
    wchar_t path[MAX_PATH];
    if (wcslen(runtime_root) + 15 >= MAX_PATH) {
        return;
    }
    wcscpy(path, runtime_root);
    wcscat(path, L"\\bootstrap.log");
    HANDLE file = CreateFileW(
        path,
        FILE_APPEND_DATA,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        NULL,
        OPEN_ALWAYS,
        FILE_ATTRIBUTE_NORMAL,
        NULL
    );
    if (file == INVALID_HANDLE_VALUE) {
        return;
    }
    DWORD written = 0;
    WriteFile(file, message, (DWORD)strlen(message), &written, NULL);
    WriteFile(file, "\r\n", 2, &written, NULL);
    CloseHandle(file);
}

static DWORD WINAPI bootstrap_python(LPVOID parameter) {
    (void)parameter;
    wchar_t module_path[MAX_PATH];
    DWORD length = GetModuleFileNameW(self_module, module_path, MAX_PATH);
    if (!length || length >= MAX_PATH) {
        return 1;
    }
    wchar_t *separator = wcsrchr(module_path, L'\\');
    if (!separator) {
        return 2;
    }
    *separator = L'\0';
    wchar_t runtime_root[MAX_PATH];
    if (wcslen(module_path) + 14 >= MAX_PATH) {
        return 3;
    }
    wcscpy(runtime_root, module_path);
    wcscat(runtime_root, L"\\SearchProbes");
    append_boot_log(runtime_root, "native bootstrap loaded");

    SetDllDirectoryW(runtime_root);
    wchar_t python_dll[MAX_PATH];
    wcscpy(python_dll, runtime_root);
    wcscat(python_dll, L"\\python313.dll");
    HMODULE python = LoadLibraryW(python_dll);
    if (!python) {
        append_boot_log(runtime_root, "could not load python313.dll");
        return 4;
    }
    py_initialize_fn py_initialize = NULL;
    py_run_simple_string_fn py_run_simple_string = NULL;
    py_finalize_ex_fn py_finalize_ex = NULL;
    LOAD_SYMBOL(py_initialize, python, "Py_Initialize");
    LOAD_SYMBOL(py_run_simple_string, python, "PyRun_SimpleString");
    LOAD_SYMBOL(py_finalize_ex, python, "Py_FinalizeEx");
    if (!py_initialize || !py_run_simple_string || !py_finalize_ex) {
        append_boot_log(runtime_root, "python313.dll is missing required embedding exports");
        return 5;
    }

    py_initialize();
    append_boot_log(runtime_root, "embedded Python initialized");
    int result = py_run_simple_string(
        "from search_probes import embedded_bootstrap\n"
        "embedded_bootstrap.main()\n"
    );
    if (result != 0) {
        append_boot_log(runtime_root, "embedded bootstrap returned an error");
    } else {
        append_boot_log(runtime_root, "embedded bootstrap stopped normally");
    }
    py_finalize_ex();
    return result == 0 ? 0 : 6;
}

__declspec(dllexport) MMRESULT WINAPI timeBeginPeriod(UINT period) {
    if (!InitOnceExecuteOnce(&real_winmm_once, load_real_winmm, NULL, NULL) ||
        !real_time_begin_period) {
        return TIMERR_NOCANDO;
    }
    return real_time_begin_period(period);
}

__declspec(dllexport) MMRESULT WINAPI timeEndPeriod(UINT period) {
    if (!InitOnceExecuteOnce(&real_winmm_once, load_real_winmm, NULL, NULL) ||
        !real_time_end_period) {
        return TIMERR_NOCANDO;
    }
    return real_time_end_period(period);
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        self_module = instance;
        DisableThreadLibraryCalls(instance);
        HANDLE thread = CreateThread(NULL, 0, bootstrap_python, NULL, 0, NULL);
        if (thread) {
            CloseHandle(thread);
        }
    }
    return TRUE;
}
