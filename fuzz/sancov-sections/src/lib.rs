#![no_std]
#![allow(non_upper_case_globals)]
#[cfg(windows)]
#[used]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".SCOV$CA")]
static mut __start___sancov_cntrs: u64 = 0;
#[cfg(windows)]
#[used]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".SCOV$CZ")]
static mut __stop___sancov_cntrs: u8 = 0;
#[cfg(windows)]
#[used]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".SCOVP$A")]
static __start___sancov_pcs: u64 = 0;
#[cfg(windows)]
#[used]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".SCOVP$Z")]
static __stop___sancov_pcs: u8 = 0;
