use solana_program::program_error::ProgramError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ShadowGuardError {
    /// vault_in amount outside [min_reserve_in, max_reserve_in].
    VaultInOutOfRange = 0x5347_0001,
    /// vault_out amount outside [min_reserve_out, max_reserve_out].
    VaultOutOutOfRange = 0x5347_0002,
}

impl From<ShadowGuardError> for ProgramError {
    fn from(e: ShadowGuardError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
