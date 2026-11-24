use std::io::Write;
#[derive(Debug, Default)]
pub struct Chunks(pub Vec<Vec<u8>>);
impl Chunks {
    pub fn len(&self) -> usize {
        self.0.iter().map(Vec::len).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
impl Write for Chunks {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.push(buf.to_vec());
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
