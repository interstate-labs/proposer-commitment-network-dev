use bls12_381::{G2Affine, G2Projective, Scalar};
use ethereum_consensus::crypto::Signature as BlsSignature;

/// Recovers the master signature from partial signatures using Lagrange interpolation.
///
/// # Arguments
///
/// * `partial_signatures` - A slice of partial signatures
/// * `identifiers` - A slice of BLS identifiers
///
/// # Returns
///
/// * `Option<BlsSignature>` - The recovered signature if successful, `None` otherwise.
pub fn recover_signature_from_shards(
    partial_signatures: &[BlsSignature],
    identifiers: &[u64],
) -> Option<BlsSignature> {
    let signatures = signatures_to_g2_projective(partial_signatures)?;
    let identifiers = identifiers.iter().map(|id| Scalar::from(*id)).collect::<Vec<_>>();
    let recovered = recover_signature_inner(&signatures, &identifiers)?;
    let recovered_bytes = G2Affine::from(recovered).to_compressed();
    BlsSignature::try_from(recovered_bytes.as_ref()).ok()
}

fn signatures_to_g2_projective(signatures: &[BlsSignature]) -> Option<Vec<G2Projective>> {
    let mut points = Vec::with_capacity(signatures.len());
    for sig in signatures {
        // Ensure that the signature is 96 bytes
        let g2_bytes = sig.as_slice();
        if g2_bytes.len() != 96 {
            return None;
        }
        // Convert the bytes into a G2Affine point
        let affine = G2Affine::from_compressed(&g2_bytes.try_into().unwrap()).into_option()?;
        // Convert to G2Projective
        let point = G2Projective::from(affine);
        points.push(point);
    }
    Some(points)
}

/// Recovers the master signature from partial signatures using Lagrange interpolation.
///
/// # Arguments
///
/// * `partial_signatures` - A slice of partial signatures (`G2Projective` points).
/// * `identifiers` - A slice of identifiers (`Scalar` field elements) corresponding to the signers.
///
/// # Returns
///
/// * `Option<G2Projective>` - The recovered signature if successful, `None` otherwise.
///
/// The Lagrange interpolation follows the reference implementation in C here:
/// https://github.com/herumi/mcl/blob/328e26f45ba565d031f9570e68e3d61836a17d7c/include/mcl/lagrange.hpp#L16
fn recover_signature_inner(
    partial_signatures: &[G2Projective],
    identifiers: &[Scalar],
) -> Option<G2Projective> {
    let k = partial_signatures.len();
    if k == 0 || k != identifiers.len() {
        return None;
    }
    if k == 1 {
        return Some(partial_signatures[0]);
    }

    // Check that all identifiers are distinct and non-zero
    for i in 0..k {
        if identifiers[i] == Scalar::zero() {
            return None;
        }
        for j in (i + 1)..k {
            if identifiers[i] == identifiers[j] {
                return None;
            }
        }
    }

    // Compute the Lagrange coefficients
    let mut lambdas = Vec::with_capacity(k);
    for i in 0..k {
        let mut num = Scalar::one();
        let mut den = Scalar::one();
        for j in 0..k {
            if i != j {
                let id_j = identifiers[j];
                let id_i = identifiers[i];

                // numerator: num *= -id_j
                let neg_id_j = -id_j; // -id_j
                num *= neg_id_j;

                // denominator: den *= id_i - id_j
                let diff = id_i - id_j;
                if diff == Scalar::zero() {
                    // identifiers are not distinct
                    return None;
                }
                den *= diff;
            }
        }
        // lambda_i = num / den
        let den_inv = den.invert().into_option()?;
        let lambda_i = num * den_inv;
        lambdas.push(lambda_i);
    }

    // Compute the recovered signature
    let mut result = G2Projective::identity();
    for i in 0..k {
        let term = partial_signatures[i] * lambdas[i];
        result += term;
    }

    Some(result)
}

#[cfg(test)]
mod tests {
    use ethereum_consensus::crypto::Signature as BlsSignature;

    use super::recover_signature_from_shards;

    #[test]
    fn test_recover_signature_from_shards() -> eyre::Result<()> {
        // Signatures obtained from the same message on 2 different Dirk nodes
        // running in a 2-of-3 threshold configuration.

        let sig_1 = alloy::hex::decode("0x92e64a646afbfc3d49343b417bde924a4ad609c288ebf857194f8967173482d839a93fa3fd70270acf73cf22b652ddcf123939b5860fe67c3b178b21fe87fe34da2100c46476147679b533110aee520f59b8ad2d1cbf613d4ff67475de75b53c")?;
        let sig_2 = alloy::hex::decode("0xabafc341960bc2d746f88d7c394164839f857f23c725252d9957d8cf27d1fe88e770f76edeaeb86a19bf5a3a9d75dd8e0f9d85bb3931bc28b715be509c0a6d37708f0c7fa36f7158a4085f47ac6ed5bbdb5cd28f2508ec5fd3fcece36ed02623")?;
        let sigs = vec![
            BlsSignature::try_from(sig_1.as_slice())?,
            BlsSignature::try_from(sig_2.as_slice())?,
        ];

        let ids = vec![1, 2];

        let recovered = recover_signature_from_shards(&sigs, &ids).expect("Failed to recover");

        // The expected signature is the master signature obtained by aggregating the partial
        // signatures from the 2 nodes. This also passed independent verification.
        let expected = BlsSignature::try_from(alloy::hex::decode("0xa36dfd65690c9ed32dddc2806bf87a0eee49fd6062ae6048b84e1a25899a74cf00132dbb0acc0b5abfd531bcc39147f50960709eafee088968cd65ab81eed3eee8c2cb0a87682e4c6cd5b71aaadf3bdcadbe5f2ddf377eb6a2942aca3347eea1")?.as_slice())?;

        assert_eq!(recovered, expected);

        Ok(())
    }
}
