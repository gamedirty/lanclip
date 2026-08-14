/// 配对验证码：SAS（Short Authentication String）风格。
/// 双方各自根据「两个设备公钥（排序后）+ 随机数」独立计算 6 位数字，
/// 用户肉眼比对是否一致，防止中间人。
pub fn pairing_code(pub_a: &[u8], binding_b: &[u8], nonce: &[u8]) -> String {
    let (x, y) = if pub_a <= binding_b { (pub_a, binding_b) } else { (binding_b, pub_a) };
    let mut h = blake3::Hasher::new();
    h.update(x);
    h.update(y);
    h.update(nonce);
    let d = h.finalize();
    let v = u32::from_be_bytes([d.as_slice()[0], d.as_slice()[1], d.as_slice()[2], d.as_slice()[3]]);
    format!("{:06}", v % 1_000_000)
}
