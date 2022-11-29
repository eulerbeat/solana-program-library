fn _flash_borrow_reserve_liquidity<'a>(
    program_id: &Pubkey,
    liquidity_amount: u64,
    source_liquidity_info: &AccountInfo<'a>,
    destination_liquidity_info: &AccountInfo<'a>,
    reserve_info: &AccountInfo<'a>,
    lending_market_info: &AccountInfo<'a>,
    lending_market_authority_info: &AccountInfo<'a>,
    sysvar_info: &AccountInfo<'a>,
    token_program_id: &AccountInfo<'a>,
) -> ProgramResult {
    ...

    // Find and validate the flash repay instruction.
    //
    // 1. Ensure the instruction is for this program
    // 2. Ensure the instruction can be unpacked into a LendingInstruction
    // 3. Ensure that the reserve for the repay matches the borrow
    // 4. Ensure that there are no other flash instructions in the rest of the transaction
    // 5. Ensure that the repay amount matches the borrow amount
    //
    // If all of these conditions are not met, the flash borrow fails.
    let mut i = current_index;
    let mut found_repay_ix = false;

    loop {
        i += 1;

        let ixn = match load_instruction_at_checked(i, sysvar_info) {
            Ok(ix) => ix,
            Err(ProgramError::InvalidArgument) => break, // out of bounds
            Err(e) => {
                return Err(e);
            }
        };

        if ixn.program_id != *program_id {
            continue;
        }

        let unpacked = LendingInstruction::unpack(ixn.data.as_slice())?;
        match unpacked {
            LendingInstruction::FlashRepayReserveLiquidity {
                liquidity_amount: repay_liquidity_amount,
                borrow_instruction_index,
            } => {
                if found_repay_ix {
                    msg!("Multiple flash repays not allowed");
                    return Err(LendingError::MultipleFlashBorrows.into());
                }
                if ixn.accounts[4].pubkey != *reserve_info.key {
                    msg!("Invalid reserve account on flash repay");
                    return Err(LendingError::InvalidFlashRepay.into());
                }
                if repay_liquidity_amount != liquidity_amount {
                    msg!("Liquidity amount for flash repay doesn't match borrow");
                    return Err(LendingError::InvalidFlashRepay.into());
                }
                if (borrow_instruction_index as usize) != current_index {
                    msg!("Borrow instruction index {} for flash repay doesn't match current index {}", borrow_instruction_index, current_index);
                    return Err(LendingError::InvalidFlashRepay.into());
                }

                found_repay_ix = true;
            }
            LendingInstruction::FlashBorrowReserveLiquidity { .. } => {
                msg!("Multiple flash borrows not allowed");
                return Err(LendingError::MultipleFlashBorrows.into());
            }
            _ => (),
        };
    }

    if !found_repay_ix {
        msg!("No flash repay found");
        return Err(LendingError::NoFlashRepayFound.into());
    }

    reserve.liquidity.borrow(Decimal::from(liquidity_amount))?;
    
    ...
}

fn _flash_repay_reserve_liquidity<'a>(
    program_id: &Pubkey,
    liquidity_amount: u64,
    borrow_instruction_index: u8,
    source_liquidity_info: &AccountInfo<'a>,
    destination_liquidity_info: &AccountInfo<'a>,
    reserve_liquidity_fee_receiver_info: &AccountInfo<'a>,
    host_fee_receiver_info: &AccountInfo<'a>,
    reserve_info: &AccountInfo<'a>,
    lending_market_info: &AccountInfo<'a>,
    user_transfer_authority_info: &AccountInfo<'a>,
    sysvar_info: &AccountInfo<'a>,
    token_program_id: &AccountInfo<'a>,
) -> ProgramResult {
    ...

    // Make sure this isnt a cpi call
    let current_index = load_current_index_checked(sysvar_info)? as usize;
    if is_cpi_call(program_id, current_index, sysvar_info)? {
        msg!("Flash Repay was called via CPI!");
        return Err(LendingError::FlashRepayCpi.into());
    }

    // validate flash borrow
    if (borrow_instruction_index as usize) > current_index {
        msg!(
            "Flash repay: borrow instruction index {} has to be less than current index {}",
            borrow_instruction_index,
            current_index
        );
        return Err(LendingError::InvalidFlashRepay.into());
    }

    let ixn = load_instruction_at_checked(borrow_instruction_index as usize, sysvar_info)?;
    if ixn.program_id != *program_id {
        msg!(
            "Flash repay: supplied instruction index {} doesn't belong to program id {}",
            borrow_instruction_index,
            *program_id
        );
        return Err(LendingError::InvalidFlashRepay.into());
    }

    let unpacked = LendingInstruction::unpack(ixn.data.as_slice())?;
    match unpacked {
        LendingInstruction::FlashBorrowReserveLiquidity {
            liquidity_amount: borrow_liquidity_amount,
        } => {
            // re-check everything here out of paranoia
            if ixn.accounts[2].pubkey != *reserve_info.key {
                msg!("Invalid reserve account on flash repay");
                return Err(LendingError::InvalidFlashRepay.into());
            }

            if liquidity_amount != borrow_liquidity_amount {
                msg!("Liquidity amount for flash repay doesn't match borrow");
                return Err(LendingError::InvalidFlashRepay.into());
            }
        }
        _ => {
            msg!("Flash repay: Supplied borrow instruction index is not a flash borrow");
            return Err(LendingError::InvalidFlashRepay.into());
        }
    };

    reserve
        .liquidity
        .repay(flash_loan_amount, flash_loan_amount_decimal)?;
    
    ...
}

fn is_cpi_call(
    program_id: &Pubkey,
    current_index: usize,
    sysvar_info: &AccountInfo,
) -> Result<bool, ProgramError> {
    // say the tx looks like:
    // ix 0
    //   - ix a
    //   - ix b
    //   - ix c
    // ix 1
    // and we call "load_current_index_checked" from b, we will get 0. And when we
    // load_instruction_at_checked(0), we will get ix 0.
    // tldr; instructions sysvar only stores top-level instructions, never CPI instructions.
    let current_ixn = load_instruction_at_checked(current_index, sysvar_info)?;

    // the current ixn must match the flash_* ix. otherwise, it's a CPI. Comparing program_ids is a
    // cheaper way of verifying this property, bc token-lending doesn't allow re-entrancy anywhere.
    if *program_id != current_ixn.program_id {
        return Ok(true);
    }

    if get_stack_height() > TRANSACTION_LEVEL_STACK_HEIGHT {
        return Ok(true);
    }
    
    ...
}
