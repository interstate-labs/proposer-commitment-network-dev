use std::{
    borrow::Borrow,
    collections::HashMap,
    fmt::Debug,
    hash::{BuildHasher, Hash, RandomState},
    ops::{Deref, DerefMut},
};

pub struct ScoreCache<
    const GET_SCORE: isize,
    const INSERT_SCORE: isize,
    const UPDATE_SCORE: isize,
    K,
    V,
    S = RandomState,
> {
    map: HashMap<K, (V, isize), S>,
    max_len: usize,
}

impl<const GET_SCORE: isize, const INSERT_SCORE: isize, const UPDATE_SCORE: isize, K, V> Default
    for ScoreCache<GET_SCORE, INSERT_SCORE, UPDATE_SCORE, K, V, RandomState>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const GET_SCORE: isize, const INSERT_SCORE: isize, const UPDATE_SCORE: isize, K, V, S> Deref
    for ScoreCache<GET_SCORE, INSERT_SCORE, UPDATE_SCORE, K, V, S>
{
    type Target = HashMap<K, (V, isize), S>;

    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl<const GET_SCORE: isize, const INSERT_SCORE: isize, const UPDATE_SCORE: isize, K, V, S> DerefMut
    for ScoreCache<GET_SCORE, INSERT_SCORE, UPDATE_SCORE, K, V, S>
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.map
    }
}

impl<
        const GET_SCORE: isize,
        const INSERT_SCORE: isize,
        const UPDATE_SCORE: isize,
        K: Debug,
        V: Debug,
        S,
    > Debug for ScoreCache<GET_SCORE, INSERT_SCORE, UPDATE_SCORE, K, V, S>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScoreCache")
            .field("map", &self.map)
            .field("max_len", &self.max_len)
            .finish()
    }
}

impl<const GET_SCORE: isize, const INSERT_SCORE: isize, const UPDATE_SCORE: isize, K, V>
    ScoreCache<GET_SCORE, INSERT_SCORE, UPDATE_SCORE, K, V, RandomState>
{
    #[inline]
    pub fn new() -> Self {
        Self {
            map: HashMap::<K, (V, isize)>::new(),
            max_len: usize::MAX,
        }
    }

    #[inline]
    pub fn with_max_len(max_len: usize) -> Self {
        Self {
            map: HashMap::<K, (V, isize)>::new(),
            max_len,
        }
    }
}
impl<const GET_SCORE: isize, const INSERT_SCORE: isize, const UPDATE_SCORE: isize, K, V, S>
    ScoreCache<GET_SCORE, INSERT_SCORE, UPDATE_SCORE, K, V, S>
where
    K: Eq + Hash,
    S: BuildHasher,
{
    #[inline]
    pub fn get<Q>(&mut self, k: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.get_mut(k).map(|(v, score)| {
            *score = score.saturating_add(GET_SCORE);
            &*v
        })
    }

    #[inline]
    pub fn get_mut<Q>(&mut self, k: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.get_mut(k).map(|(v, score)| {
            *score = score.saturating_add(UPDATE_SCORE);
            v
        })
    }

    #[inline]
    pub fn insert(&mut self, k: K, v: V) -> Option<V> {
        self.clear_stales();
        self.map.insert(k, (v, INSERT_SCORE)).map(|(v, _)| v)
    }
}

impl<const GET_SCORE: isize, const INSERT_SCORE: isize, const UPDATE_SCORE: isize, K, V, S>
    ScoreCache<GET_SCORE, INSERT_SCORE, UPDATE_SCORE, K, V, S>
{
    #[inline]
    fn clear_stales(&mut self) {
        let mut i = 0;
        while self.len() >= self.max_len {
            self.map.retain(|_, (_, score)| *score > i);
            i += 1;
        }
    }
}