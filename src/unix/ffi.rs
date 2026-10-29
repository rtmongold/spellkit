use std::ffi::{c_char, c_int};
use std::marker::{PhantomData, PhantomPinned};

#[repr(C)]
pub(super) struct Hunhandle {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

unsafe extern "C" {
    pub(super) fn Hunspell_create(affpath: *const c_char, dpath: *const c_char) -> *mut Hunhandle;
    pub(super) fn Hunspell_destroy(handle: *mut Hunhandle);
    pub(super) fn Hunspell_spell(handle: *mut Hunhandle, word: *const c_char) -> c_int;
    pub(super) fn Hunspell_suggest(
        handle: *mut Hunhandle,
        list: *mut *mut *mut c_char,
        word: *const c_char,
    ) -> c_int;
    pub(super) fn Hunspell_add(handle: *mut Hunhandle, word: *const c_char) -> c_int;
    pub(super) fn Hunspell_free_list(
        handle: *mut Hunhandle,
        list: *mut *mut *mut c_char,
        count: c_int,
    );
}
