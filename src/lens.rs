use druid::Lens;

#[derive(Debug, Copy, Clone)]
pub struct ReadOnly<Get> {
    get: Get,
}

impl<Get> ReadOnly<Get> {
    /// Construct a mapping
    ///
    /// See also `LensExt::map`
    pub fn new<A: ?Sized, B>(get: Get) -> Self
    where
        Get: Fn(&A) -> B,
    {
        Self { get }
    }
}

impl<A: ?Sized, B, Get> Lens<A, B> for ReadOnly<Get>
where
    Get: Fn(&A) -> B,
{
    fn with<V, F: FnOnce(&B) -> V>(&self, data: &A, f: F) -> V {
        f(&(self.get)(data))
    }

    fn with_mut<V, F: FnOnce(&mut B) -> V>(&self, data: &mut A, f: F) -> V {
        let mut temp = (self.get)(data);
        let x = f(&mut temp);
        x
    }
}
