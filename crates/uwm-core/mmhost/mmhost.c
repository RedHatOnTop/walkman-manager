/* mmhost.exe: runs the 12-tone analysis of the user's own Music Center for PC install
 * (AVLib/MMLib11.dll, COM class MusicAnalysis2), natively on Windows or under Wine. The
 * DLL is loaded from the path given; nothing of Sony's is part of this program.
 *
 *   mmhost.exe PATH\MMLib11.dll < jobs > results
 *
 * stdin, per job:  "UWMJ" u32 rate, u32 channels, u32 bits, then chunks of PCM
 *                  (interleaved little-endian), each u32 length + bytes, ended by length 0
 * stdout, per job: "UWMR" i32 hr, u32 length, then the SMFMF block (result 89)
 *
 * Each chunk goes to the engine as one InputPCM call, the last one flagged as last. The
 * engine analyses on a thread of its own and one of its fields depends on how the PCM
 * is cut, so the sender picks the chunk size (Music Center's blocks come back byte for
 * byte with 4096).
 *
 * Build: i686-w64-mingw32-gcc -O2 -municode -s -o mmhost.exe mmhost.c -lole32 -loleaut32
 */
#include <fcntl.h>
#include <io.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <windows.h>

typedef HRESULT(WINAPI *GetClassObject)(REFCLSID, REFIID, void **);
typedef HRESULT(WINAPI *Fn0)(void *);
typedef HRESULT(WINAPI *Fn1)(void *, uintptr_t);
typedef HRESULT(WINAPI *Fn2)(void *, uintptr_t, uintptr_t);
typedef HRESULT(WINAPI *Fn3)(void *, uintptr_t, uintptr_t, uintptr_t);
typedef HRESULT(WINAPI *Fn4)(void *, uintptr_t, uintptr_t, uintptr_t, uintptr_t);
typedef HRESULT(WINAPI *Fn5)(void *, uintptr_t, uintptr_t, uintptr_t, uintptr_t, uintptr_t);

static const GUID CLSID_MusicAnalysis2 = {
    0x8DE9ED2D, 0x8CFB, 0x4b33, {0xB8, 0xE3, 0xCE, 0xE0, 0x6F, 0x29, 0x03, 0xE3}};
static const GUID IID_IMusicAnalysis2 = {
    0x0959C485, 0xA191, 0x4508, {0xA3, 0x38, 0x9E, 0x2B, 0x38, 0x13, 0xA1, 0x66}};
static const GUID IID_ClassFactory = {
    0x00000001, 0x0000, 0x0000, {0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46}};

enum { SMFMF_RESULT = 89, MAX_CHUNK = 1 << 24 };

/* Not an HRESULT the engine returns: the job's PCM did not arrive whole. */
#define BAD_STREAM ((HRESULT)0xA0550001)

static void *slot(void *obj, int i) { return (*(void ***)obj)[i]; }

static int take(void *p, size_t n) { return fread(p, 1, n, stdin) == n; }

static void reply(int32_t hr, const void *data, uint32_t n) {
    fwrite("UWMR", 1, 4, stdout);
    fwrite(&hr, 4, 1, stdout);
    fwrite(&n, 4, 1, stdout);
    if (n)
        fwrite(data, 1, n, stdout);
    fflush(stdout);
}

typedef struct {
    uint8_t *data;
    uint32_t len, cap;
} Chunk;

/* Reads one chunk; returns 0 at the end marker, -1 on a broken stream. */
static int next_chunk(Chunk *c) {
    uint32_t n;
    if (!take(&n, 4) || n > MAX_CHUNK)
        return -1;
    if (n > c->cap) {
        uint8_t *p = realloc(c->data, n);
        if (!p)
            return -1;
        c->data = p;
        c->cap = n;
    }
    c->len = n;
    return n && !take(c->data, n) ? -1 : (int)(n != 0);
}

static HRESULT analyse(void *factory, const uint32_t fmt[3], Chunk *cur, Chunk *ahead, int *in_step) {
    void *engine = NULL, *result = NULL;
    HRESULT hr = ((Fn3)slot(factory, 3))(factory, 0, (uintptr_t)&IID_IMusicAnalysis2,
                                         (uintptr_t)&engine);
    /* SetInputPCMFormat(channels, rate, bits) */
    if (!hr)
        hr = ((Fn3)slot(engine, 16))(engine, fmt[1], fmt[0], fmt[2]);
    /* SetParameter(4, VARIANT{VT_BOOL, 1}), as Music Center sets it */
    if (!hr)
        hr = ((Fn5)slot(engine, 13))(engine, 4, VT_BOOL, 0, 1, 0);
    if (!hr)
        hr = ((Fn0)slot(engine, 7))(engine); /* Run */
    /* The engine runs one chunk behind the stream so that the last one can be flagged;
     * after a failure the rest of the job is still read, to stay in step. */
    int have = next_chunk(cur);
    while (have > 0) {
        int more = next_chunk(ahead);
        if (more < 0) {
            have = -1;
            break;
        }
        /* InputPCM(buf, size, last, async) */
        if (!hr)
            hr = ((Fn4)slot(engine, 15))(engine, (uintptr_t)cur->data, cur->len, !more, 0);
        Chunk t = *cur;
        *cur = *ahead;
        *ahead = t;
        have = more;
    }
    *in_step = have == 0;
    if (!*in_step && !hr)
        hr = BAD_STREAM;
    if (!hr)
        hr = ((Fn1)slot(engine, 17))(engine, (uintptr_t)&result); /* GetResult */
    if (!hr) {
        VARIANT v;
        VariantInit(&v);
        /* IAnalysisResult2::GetValue(id, VARIANT *) */
        hr = ((Fn2)slot(result, 8))(result, SMFMF_RESULT, (uintptr_t)&v);
        if (!hr && v.vt == (VT_ARRAY | VT_UI1) && v.parray) {
            SAFEARRAY *sa = v.parray;
            reply(0, sa->pvData, sa->rgsabound[0].cElements * sa->cbElements);
        } else if (!hr) {
            hr = E_UNEXPECTED;
        }
        VariantClear(&v);
        ((Fn0)slot(result, 2))(result);
    }
    if (engine)
        ((Fn0)slot(engine, 2))(engine);
    return hr;
}

int wmain(int argc, wchar_t **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: mmhost.exe MMLib11.dll < jobs > results\n");
        return 2;
    }
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
    CoInitializeEx(NULL, COINIT_APARTMENTTHREADED);
    HMODULE dll = LoadLibraryW(argv[1]);
    if (!dll) {
        fprintf(stderr, "mmhost: cannot load the engine (error %lu)\n", GetLastError());
        return 1;
    }
    GetClassObject get = (GetClassObject)GetProcAddress(dll, "DllGetClassObject");
    void *factory = NULL;
    HRESULT hr = get ? get(&CLSID_MusicAnalysis2, &IID_ClassFactory, &factory) : E_NOINTERFACE;
    if (hr) {
        fprintf(stderr, "mmhost: the DLL has no MusicAnalysis2 class (hr %#lx)\n", (unsigned long)hr);
        return 1;
    }
    Chunk cur = {0}, ahead = {0};
    char magic[4];
    uint32_t fmt[3];
    while (take(magic, 4)) {
        if (memcmp(magic, "UWMJ", 4) || !take(fmt, sizeof fmt)) {
            fprintf(stderr, "mmhost: bad job header\n");
            return 1;
        }
        int in_step = 1;
        hr = analyse(factory, fmt, &cur, &ahead, &in_step);
        if (hr)
            reply((int32_t)hr, NULL, 0);
        if (!in_step) {
            fprintf(stderr, "mmhost: the PCM stream broke off\n");
            return 1;
        }
    }
    ((Fn0)slot(factory, 2))(factory);
    return 0;
}
