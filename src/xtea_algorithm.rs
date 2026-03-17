pub fn xtea_encrypt(value: &[u32; 2], key: &[u32; 4]) -> [u32; 2] {
    let mut v0 = value[0];
    let mut v1 = value[1];
    let delta: u32 = 0x9E3779B9;
    let mut sum: u32 = 0;

    for _ in 0..32 {
        sum = sum.wrapping_add(delta);
        v0 = v0.wrapping_add(
            ((v1 << 4).wrapping_add(key[0]))
                ^ (v1.wrapping_add(sum))
                ^ ((v1 >> 5).wrapping_add(key[1])),
        );
        v1 = v1.wrapping_add(
            ((v0 << 4).wrapping_add(key[2]))
                ^ (v0.wrapping_add(sum))
                ^ ((v0 >> 5).wrapping_add(key[3])),
        );
    }

    [v0, v1]
}

pub fn xtea_decrypt(value: &[u32; 2], key: &[u32; 4]) -> [u32; 2] {
    let mut v0 = value[0];
    let mut v1 = value[1];
    let delta: u32 = 0x9E3779B9;
    let mut sum: u32 = delta.wrapping_mul(32);

    for _ in 0..32 {
        v1 = v1.wrapping_sub(
            ((v0 << 4).wrapping_add(key[2]))
                ^ (v0.wrapping_add(sum))
                ^ ((v0 >> 5).wrapping_add(key[3])),
        );
        v0 = v0.wrapping_sub(
            ((v1 << 4).wrapping_add(key[0]))
                ^ (v1.wrapping_add(sum))
                ^ ((v1 >> 5).wrapping_add(key[1])),
        );
        sum = sum.wrapping_sub(delta);
    }

    [v0, v1]
}
