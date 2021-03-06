use fnv::{FnvHashMap, FnvHashSet};

use util;

use regex::Regex;
use std::borrow::Cow;

pub const MISS_WORD_REL: u64 = 6666;
pub const WORDS_PER_QUERY: usize = 15;

const PUNCT_SYMBOLS: &str = "[/@#!,'?:();.+-_]";

macro_rules! vec_push_str {
    // Base case:
    ($v:ident, $w:expr) => (
        $v.push_str($w);
    );

    ($v:ident, $w1:expr, $($w2:expr),+) => (
        $v.push_str($w1);
        $v.push_str(" ");
        vec_push_str!($v, $($w2),+);
    )
}

macro_rules! bow2 {
    ($v: ident, $w1: expr, $w2: expr) => {{
        if $w1 < $w2 {
            vec_push_str!($v, $w1, $w2);
        } else {
            vec_push_str!($v, $w2, $w1);
        }

        $v
    }};
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum ParseMode {
    Index,
    Search,
}

#[inline]
fn bow2(w1: &str, w2: &str) -> String {
    let mut v = String::with_capacity(w1.len() + w2.len() + 1);

    bow2!(v, w1, w2)
}

#[inline]
fn bow3(w1: &str, w2: &str, w3: &str) -> String {
    let mut v = String::with_capacity(w1.len() + w2.len() + w3.len() + 2);
    if w1 < w2 && w1 < w3 {
        v.push_str(w1);
        v.push_str(" ");

        return bow2!(v, w2, w3);
    } else if w2 < w1 && w2 < w3 {
        v.push_str(w2);
        v.push_str(" ");

        return bow2!(v, w1, w3);
    } else {
        v.push_str(w3);
        v.push_str(" ");

        return bow2!(v, w1, w2);
    }
}

#[inline]
fn update(
    ngrams: &mut Vec<String>,
    relevs: &mut Vec<f32>,
    ngrams_ids: &mut FnvHashMap<String, Vec<usize>>,
    ngram: String,
    relev: f32,
    indices: Vec<usize>,
) {
    if !ngrams_ids.contains_key(&ngram) {
        relevs.push(relev);
        ngrams.push(ngram.clone());
        ngrams_ids.insert(ngram.clone(), indices);
    }
}

#[inline]
fn update_linked(
    words_indices: &Vec<usize>,
    words: &Vec<String>,
    linked_idx: &mut FnvHashSet<usize>,
    linked_words: &mut FnvHashSet<String>,
) {
    for idx in words_indices {
        linked_idx.insert(*idx);
        linked_words.insert(words[*idx].clone());
    }
}

#[inline]
pub fn u8_find_and_replace<'a, S: Into<Cow<'a, str>>>(input: S) -> Cow<'a, str> {
    lazy_static! {
        static ref PUNCT_RE: Regex = Regex::new(PUNCT_SYMBOLS).unwrap();
    }
    let input = input.into();
    if let Some(mat) = PUNCT_RE.find(&input) {
        let start = mat.start();
        let len = input.len();
        let mut output: Vec<u8> = Vec::with_capacity(len + len / 2);
        output.extend_from_slice(input[0..start].as_bytes());
        let rest = input[start..].bytes();
        for c in rest {
            match c {
                b'!' | b',' | b'?' | b':' | b'\'' => (),
                b'#' | b'@' | b'(' | b')' | b';' | b'.' | b'/' | b'+' | b'-' | b'_' => {
                    output.extend_from_slice(b" ")
                }
                _ => output.push(c),
            }
        }
        Cow::Owned(unsafe { String::from_utf8_unchecked(output) })
    } else {
        input
    }
}

#[inline]
pub fn u8_normalize_umlauts<'a, S: Into<Cow<'a, str>>>(input: S) -> Cow<'a, str> {
    lazy_static! {
        static ref REGEX: Regex = Regex::new("[ßöäü]").unwrap();
    }
    let input = input.into();

    if REGEX.is_match(&input) {
        let mut last_match = 0;
        let len = input.len();
        let matches = REGEX.find_iter(&input);
        let mut output: Vec<u8> = Vec::with_capacity(len + len / 2);
        for m in matches {
            output.extend_from_slice(&input[last_match..m.start()].as_bytes());
            match &input[m.start()..m.end()] {
                "ß" => output.extend_from_slice("ss".as_bytes()),
                "ä" => output.extend_from_slice("ae".as_bytes()),
                "ü" => output.extend_from_slice("ue".as_bytes()),
                "ö" => output.extend_from_slice("oe".as_bytes()),
                _ => unreachable!(),
            }
            last_match = m.end();
        }
        output.extend_from_slice(&input[last_match..].as_bytes());
        Cow::Owned(unsafe { String::from_utf8_unchecked(output) })
    } else {
        input
    }
}

#[inline]
pub fn separate_digits<'a, S: Into<Cow<'a, str>>>(input: S) -> Cow<'a, str> {
    lazy_static! {
        // splits numbers from the rest of the string
        static ref RE_DIG: regex::Regex = regex::Regex::new(r"(?x)
            (?P<b>[[:alpha:]]{2,}|\s|^) # (at least 2 letters in front OR a space OR it's the beginning) AND
            (?P<d>\d{2,})               # at least 2 digits
        ").unwrap();
    }

    Cow::Owned(RE_DIG.replace_all(&input.into(), "$b $d $e").to_string())
}

#[inline]
pub fn normalize(query: &str) -> String {
    separate_digits(u8_normalize_umlauts(
        u8_find_and_replace(query).to_lowercase(),
    ))
    .trim()
    .to_string()
}

#[inline]
fn suffix_words(words: &mut Vec<String>, suffix_letters: &mut Vec<(usize, String)>) -> Vec<String> {
    let mut word_idx = 0;
    let mut suffixed_words: Vec<String> = vec![];

    words.reverse();
    suffix_letters.reverse();

    while let Some(w) = words.pop() {
        let mut sw = String::with_capacity(w.len() + suffix_letters.len());
        sw.push_str(&w);
        while let Some((i, suffix)) = suffix_letters.last() {
            if *i != word_idx {
                break;
            } else {
                sw.push_str(suffix);
                suffix_letters.pop();
            }
        }
        suffixed_words.push(sw);
        word_idx += 1;
    }

    // if still relatively sparse, join into one word vec:
    //  @xel en e x -> [xelenex] instead of [xel, enex]
    //  @xe l en e x -> [xelenex] instead of [xel, enex]
    if suffixed_words.len() > 1 && suffixed_words.iter().all(|w| w.len() <= 4) {
        suffixed_words = vec![suffixed_words.join("")];
    }

    suffixed_words
}

#[inline]
fn suffix_synonyms(
    words: &Vec<String>,
    suffix_letters: &mut Vec<(usize, String)>,
    synonyms: &mut FnvHashMap<usize, String>,
) {
    if suffix_letters.is_empty() {
        return;
    }

    for (word_idx, word) in words.iter().enumerate() {
        let mut synonym = String::with_capacity(word.len() + suffix_letters.len());
        synonym.push_str(word);
        while let Some((i, suffix)) = suffix_letters.last() {
            if *i != word_idx {
                break;
            } else {
                synonym.push_str(suffix);
                suffix_letters.pop();
            }
        }
        if word.len() < synonym.len() {
            synonyms.insert(word_idx, synonym);
        }
    }
}

#[inline]
fn word_synonyms(
    words: &Vec<String>,
    synonyms: &mut FnvHashMap<usize, String>,
    synonyms_dict: &Option<FnvHashMap<String, String>>,
) {
    let words_set: FnvHashSet<String> = words.iter().map(|w| w.to_string()).collect();
    if let Some(syn_dict) = synonyms_dict {
        for (word_idx, word) in words.iter().enumerate() {
            if !synonyms.contains_key(&word_idx) {
                if let Some(syn) = syn_dict.get(word) {
                    if !words_set.contains(syn) {
                        synonyms.insert(word_idx, syn.to_string());
                    }
                }
            }
        }
    }
}

#[inline]
fn get_norm_query_vec(
    query: &str,
    synonyms_dict: &Option<FnvHashMap<String, String>>,
    mode: ParseMode,
) -> (Vec<String>, FnvHashMap<usize, String>) {
    let mut suffix_letters: Vec<(usize, String)> = Vec::with_capacity(WORDS_PER_QUERY - 1);
    let mut synonyms: FnvHashMap<usize, String> = FnvHashMap::default();

    let mut words_cnt: usize = 0;
    let mut words = normalize(query)
        .split(" ")
        .enumerate()
        .filter(|(i, word)| {
            let word = word.trim();
            if word.len() > 1 {
                words_cnt += 1;
                return true;
            }

            if let Some(c) = word.chars().next() {
                if *i == 0 || c.is_digit(10) {
                    words_cnt += 1;
                    return true;
                }

                if !c.is_alphabetic() {
                    return false;
                }

                suffix_letters.push((words_cnt - 1, word.to_string()));

                return false;
            }

            return false;
        })
        .map(|(_, w)| w.to_string())
        .collect::<Vec<String>>();
    words.truncate(WORDS_PER_QUERY);

    if words.is_empty() {
        return (words, synonyms);
    }

    // join sparse words e.g.: '@x e l e n e x', '@xe l e n e x' etc.
    if (words.len() > 2 || !suffix_letters.is_empty()) && words.iter().all(|w| w.len() <= 4) {
        words = suffix_words(&mut words, &mut suffix_letters);
    } else if mode == ParseMode::Search {
        suffix_synonyms(&mut words, &mut suffix_letters, &mut synonyms);
        word_synonyms(&mut words, &mut synonyms, &synonyms_dict);
    }

    (words, synonyms)
}

#[inline]
pub fn get_words_relevances(
    query: &str,
    tr_map: &fst::Map,
    stopwords: &FnvHashSet<String>,
    synonyms: &FnvHashMap<usize, String>,
    toponyms: &Option<fst::Set>,
    synonyms_dict: &Option<FnvHashMap<String, String>>,
    mode: ParseMode,
) -> FnvHashMap<String, f32> {
    let (words, _) = get_norm_query_vec(query, &None, mode);
    let (_, _, relevs, _, _) =
        index_words(&words, tr_map, stopwords, synonyms, toponyms, synonyms_dict);

    words
        .iter()
        .zip(relevs.iter())
        .map(|(w, r)| (w.to_string(), *r))
        .collect::<FnvHashMap<String, f32>>()
}

#[inline]
fn index_words(
    words: &Vec<String>,
    tr_map: &fst::Map,
    stopwords: &FnvHashSet<String>,
    synonyms: &FnvHashMap<usize, String>,
    toponyms: &Option<fst::Set>,
    synonyms_dict: &Option<FnvHashMap<String, String>>,
) -> (
    Vec<usize>,
    Vec<usize>,
    Vec<f32>,
    Vec<usize>,
    FnvHashSet<usize>,
) {
    let words_len = words.len();
    let mut rels: Vec<f32> = Vec::with_capacity(words_len);
    let mut stop_vec: Vec<usize> = Vec::with_capacity(words_len);
    let mut numerics: FnvHashSet<usize> = FnvHashSet::default();
    let (mut numeric_rel, mut numeric) = (0.0, words_len);
    let (mut toponym_rel, mut toponym) = (0.0, words_len);
    let mut word_vec: Vec<usize> = Vec::with_capacity(words_len);
    let mut seen_words: FnvHashSet<String> = FnvHashSet::default();

    let mut min_rel = std::f32::MAX;
    let mut min_word_idx: usize = 0;
    let stop_word_thresh: f32 = 1.0 / (2 * words_len + 1) as f32;

    let mut norm: f32 = 0.0;
    for (i, word) in words.iter().enumerate() {
        let mut rel = tr_map.get(word).unwrap_or(MISS_WORD_REL) as f32;

        if stopwords.contains(word) || word.len() == 1 {
            rel = 0.5 * rel;
            stop_vec.push(i);
            rels.push(rel);
            norm += rel;
        } else {
            if synonyms.contains_key(&i) {
                let syn = synonyms.get(&i).unwrap();
                rel = util::max(tr_map.get(syn).unwrap_or(MISS_WORD_REL) as f32, rel);
            }

            if !seen_words.contains(word) {
                norm += rel;
            }

            if word.chars().any(char::is_numeric) {
                numerics.insert(i);
                if rel > numeric_rel {
                    numeric_rel = rel;
                    numeric = i;
                }
            }

            if let Some(toponyms) = toponyms {
                if toponyms.contains(word) && rel > toponym_rel {
                    toponym_rel = rel;
                    toponym = i;
                }
            }

            rels.push(rel);
            word_vec.push(i);
        }

        if rel < min_rel {
            min_rel = rel;
            min_word_idx = i;
        }

        if seen_words.contains(word) {
            continue;
        }

        // record seen words
        seen_words.insert(word.to_string());
        if let Some(syn_dict) = synonyms_dict {
            if let Some(syn) = syn_dict.get(word) {
                seen_words.insert(syn.to_string());

                if let Some(toponyms) = toponyms {
                    if toponyms.contains(syn) && rel > toponym_rel {
                        toponym_rel = rel;
                        toponym = i;
                    }
                }
            }
        }
    }

    let rels: Vec<f32> = rels.into_iter().map(|r| r / norm).collect();

    if stop_vec.is_empty() && words_len > 3 && rels[min_word_idx] < stop_word_thresh {
        stop_vec.push(min_word_idx);
        word_vec.remove(min_word_idx);
    }

    let mut must_have: Vec<usize> = Vec::with_capacity(2);

    if numeric < words_len {
        must_have.push(numeric)
    }

    if toponym < words_len && toponym != numeric {
        must_have.push(toponym)
    }

    (word_vec, stop_vec, rels, must_have, numerics)
}

#[derive(Debug)]
pub struct StopNgram {
    pub ngram: String,
    pub relev: f32,
    pub word_indices: Vec<usize>,
}

impl StopNgram {
    #[inline]
    pub fn new(ngram: String, relev: f32, word_indices: Vec<usize>) -> Self {
        StopNgram {
            ngram: ngram,
            relev: relev,
            word_indices: word_indices,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.word_indices.len()
    }
}

#[derive(Debug)]
pub struct StopNgrams {
    ngrams: Vec<StopNgram>,
    synonyms: FnvHashMap<usize, Vec<StopNgram>>,
    mode: ParseMode,
}

use std::ops::Index;
use std::str;
impl Index<usize> for StopNgrams {
    type Output = StopNgram;

    #[inline]
    fn index(&self, idx: usize) -> &StopNgram {
        &self.ngrams[idx]
    }
}

impl StopNgrams {
    pub fn with_capacity(capacity: usize, mode: ParseMode) -> Self {
        StopNgrams {
            ngrams: Vec::with_capacity(capacity),
            synonyms: FnvHashMap::default(),
            mode: mode,
        }
    }

    #[inline]
    pub fn update(
        &mut self,
        words: &Vec<String>,
        rels: &Vec<f32>,
        indices: Vec<usize>,
        synonyms: &FnvHashMap<usize, String>,
    ) {
        let ngram_idx = self.ngrams.len();

        if indices.len() == 3 {
            let (i, j, k) = (indices[0], indices[1], indices[2]);

            if self.mode == ParseMode::Search {
                if let Some(syn) = synonyms.get(&i) {
                    let ngram_syns = self.synonyms.entry(ngram_idx).or_insert(vec![]);
                    ngram_syns.push(StopNgram::new(
                        bow3(&syn, &words[j], &words[k]),
                        rels[i] + rels[j] + rels[k],
                        indices.clone(),
                    ));
                };

                if let Some(syn) = synonyms.get(&j) {
                    let ngram_syns = self.synonyms.entry(ngram_idx).or_insert(vec![]);
                    ngram_syns.push(StopNgram::new(
                        bow3(&words[i], &syn, &words[k]),
                        rels[i] + rels[j] + rels[k],
                        indices.clone(),
                    ));
                };

                if let Some(syn) = synonyms.get(&k) {
                    let ngram_syns = self.synonyms.entry(ngram_idx).or_insert(vec![]);
                    ngram_syns.push(StopNgram::new(
                        bow3(&words[i], &words[j], &syn),
                        rels[i] + rels[j] + rels[k],
                        indices.clone(),
                    ));
                };
            }

            self.ngrams.push(StopNgram::new(
                bow3(&words[i], &words[j], &words[k]),
                rels[i] + rels[j] + rels[k],
                indices,
            ));
        } else if indices.len() == 2 {
            let (i, j) = (indices[0], indices[1]);

            if self.mode == ParseMode::Search {
                if let Some(syn) = synonyms.get(&i) {
                    let ngram_syns = self.synonyms.entry(ngram_idx).or_insert(vec![]);
                    ngram_syns.push(StopNgram::new(
                        bow2(&syn, &words[j]),
                        rels[i] + rels[j],
                        indices.clone(),
                    ));
                };

                if let Some(syn) = synonyms.get(&j) {
                    let ngram_syns = self.synonyms.entry(ngram_idx).or_insert(vec![]);
                    ngram_syns.push(StopNgram::new(
                        bow2(&words[i], &syn),
                        rels[i] + rels[j],
                        indices.clone(),
                    ));
                };
            };

            self.ngrams.push(StopNgram::new(
                bow2(&words[i], &words[j]),
                rels[i] + rels[j],
                indices,
            ));
        } else if indices.len() == 1 {
            let i = indices[0];

            if self.mode == ParseMode::Search {
                if let Some(syn) = synonyms.get(&i) {
                    let ngram_syns = self.synonyms.entry(ngram_idx).or_insert(vec![]);
                    ngram_syns.push(StopNgram::new(syn.to_string(), rels[i], indices.clone()));
                };
            };

            self.ngrams
                .push(StopNgram::new(words[i].to_string(), rels[i], indices));
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.ngrams.len()
    }
}

#[inline]
pub fn get_stop_ngrams(
    words: &Vec<String>,
    rels: &Vec<f32>,
    word_idx: &mut Vec<usize>,
    stop_idx: &Vec<usize>,
    synonyms: &FnvHashMap<usize, String>,
    mode: ParseMode,
) -> StopNgrams {
    let words_len = words.len();
    let mut stop_ngrams: StopNgrams = StopNgrams::with_capacity(words_len, mode);
    let last_word_idx = words_len - 1;

    word_idx.reverse();

    let stop_idx_set: FnvHashSet<usize> = stop_idx.iter().cloned().collect();
    let mut skip_idx: FnvHashSet<usize> = FnvHashSet::default();
    let mut linked_idx: FnvHashSet<usize> = FnvHashSet::default();
    let mut linked_words: FnvHashSet<String> = FnvHashSet::default();

    for i in stop_idx.iter() {
        if skip_idx.contains(i) {
            continue;
        }

        // begins with stopword
        if *i == 0 {
            let j = *i + 1;
            skip_idx.insert(j);
            update_linked(&vec![*i, j], &words, &mut linked_idx, &mut linked_words);
            if !stop_idx_set.contains(&j) || j == last_word_idx {
                stop_ngrams.update(&words, &rels, vec![*i, j], &synonyms);
            } else {
                update_linked(&vec![j + 1], &words, &mut linked_idx, &mut linked_words);
                if stop_idx_set.contains(&(j + 1)) {
                    skip_idx.insert(j + 1);
                }
                stop_ngrams.update(&words, &rels, vec![*i, j, j + 1], &synonyms);
            }

        // stopword in between
        } else if *i < last_word_idx {
            let j = *i - 1;
            let k = *i + 1;

            // push all single-no-stop words
            if !word_idx.is_empty() {
                let mut next_i = word_idx.pop().unwrap();
                while next_i < j && !linked_idx.contains(&next_i) && !word_idx.is_empty() {
                    stop_ngrams.update(&words, &rels, vec![next_i], &synonyms);
                    update_linked(&vec![next_i], &words, &mut linked_idx, &mut linked_words);
                    next_i = word_idx.pop().unwrap();
                }
            }

            // only k is a stopword
            if !stop_idx_set.contains(&j) && stop_idx_set.contains(&k) {
                // take k+1 if k is not the last word and tr(k+1) > tr(j)
                if k < last_word_idx && !stop_idx_set.contains(&(k + 1)) && rels[k + 1] >= rels[j] {
                    if !linked_idx.contains(&j) {
                        update_linked(&vec![j], &words, &mut linked_idx, &mut linked_words);
                        stop_ngrams.update(&words, &rels, vec![j], &synonyms);
                    }

                    skip_idx.insert(k);
                    skip_idx.insert(k + 1);
                    update_linked(
                        &vec![k, *i, k + 1],
                        &words,
                        &mut linked_idx,
                        &mut linked_words,
                    );

                    stop_ngrams.update(&words, &rels, vec![*i, k, k + 1], &synonyms);
                } else if k < last_word_idx && rels[j] > rels[k + 1] && !linked_idx.contains(&j) {
                    update_linked(&vec![*i, j], &words, &mut linked_idx, &mut linked_words);
                    stop_ngrams.update(&words, &rels, vec![j, *i], &synonyms);
                } else {
                    skip_idx.insert(k);
                    update_linked(&vec![k, *i], &words, &mut linked_idx, &mut linked_words);

                    stop_ngrams.update(&words, &rels, vec![*i, k], &synonyms);
                }

            // only j is a stopword
            } else if stop_idx_set.contains(&j) && !stop_idx_set.contains(&k) {
                update_linked(&vec![k, *i], &words, &mut linked_idx, &mut linked_words);
                stop_ngrams.update(&words, &rels, vec![*i, k], &synonyms);

            // both j & k are stopwords, since j is linked, take k
            } else if stop_idx_set.contains(&j) && stop_idx_set.contains(&k) {
                skip_idx.insert(k);
                update_linked(&vec![k, *i], &words, &mut linked_idx, &mut linked_words);
                if k == last_word_idx || stop_idx_set.contains(&(k + 1)) {
                    stop_ngrams.update(&words, &rels, vec![*i, k], &synonyms);

                // take also k+1 if it's not a stop word
                } else {
                    skip_idx.insert(k + 1);
                    update_linked(&vec![k + 1], &words, &mut linked_idx, &mut linked_words);
                    stop_ngrams.update(&words, &rels, vec![*i, k, k + 1], &synonyms);
                }

            // neither j, nor k are stopwords
            } else {
                if linked_words.contains(&words[k])
                    || !linked_idx.contains(&j)
                        && (words[j].len() >= 4 * words[k].len()
                            || words[*i].len() == 1
                                && (rels[j] > rels[k] || words[k].chars().all(char::is_numeric)))
                {
                    update_linked(&vec![*i, j], &words, &mut linked_idx, &mut linked_words);
                    stop_ngrams.update(&words, &rels, vec![j, *i], &synonyms);
                } else {
                    update_linked(&vec![k, *i], &words, &mut linked_idx, &mut linked_words);
                    if !linked_idx.contains(&j) {
                        stop_ngrams.update(&words, &rels, vec![j], &synonyms);
                        update_linked(&vec![j], &words, &mut linked_idx, &mut linked_words);
                    }
                    stop_ngrams.update(&words, &rels, vec![*i, k], &synonyms);
                }
            }

        // ends with stopword
        } else {
            let j = *i - 1;
            // push all single-no-stop words, TODO macro
            if !word_idx.is_empty() {
                let mut next_i = word_idx.pop().unwrap();
                while next_i < j && !linked_idx.contains(&next_i) && !word_idx.is_empty() {
                    stop_ngrams.update(&words, &rels, vec![next_i], &synonyms);
                    update_linked(&vec![next_i], &words, &mut linked_idx, &mut linked_words);
                    next_i = word_idx.pop().unwrap();
                }
            }

            if !linked_idx.contains(&j) {
                update_linked(&vec![*i, j], &words, &mut linked_idx, &mut linked_words);
                stop_ngrams.update(&words, &rels, vec![j, *i], &synonyms);

            // previous word is in ngram, add this word to it and exit
            } else {
                update_linked(&vec![*i], &words, &mut linked_idx, &mut linked_words);

                let mut stop_ngram = stop_ngrams.ngrams.pop().unwrap();
                stop_ngram.ngram = bow2(&stop_ngram.ngram, &words[*i]);
                stop_ngram.relev += rels[*i];
                stop_ngram.word_indices.push(*i);

                stop_ngrams.ngrams.push(stop_ngram);
            }
        }
    }

    while let Some(next_i) = word_idx.pop() {
        if !linked_idx.contains(&next_i) {
            stop_ngrams.update(&words, &rels, vec![next_i], &synonyms);
        }
    }

    stop_ngrams
}

use std::cmp::{Ordering, PartialOrd};

#[inline]
fn must_have_top_word(
    must_have: &FnvHashSet<usize>,
    ids_words_rels: &Vec<(usize, String, f32)>,
    relevance_threshold: f32,
) -> bool {
    must_have.contains(&ids_words_rels[1].0)
        || ids_words_rels[1].2 < relevance_threshold * ids_words_rels[0].2
}

#[inline]
fn find_must_have_words(
    ids_words_rels: &Vec<(usize, String, f32)>,
    must_have: &Vec<usize>,
    numerics: &FnvHashSet<usize>,
    words_len: usize,
    word_rel_thresh: f32,
) -> (usize, Vec<usize>) {
    let mut must_word_idx: usize = words_len;
    if must_have.len() > 0 {
        must_word_idx = must_have[0];
    }
    let mut must_have: FnvHashSet<usize> = must_have.iter().cloned().collect();

    if ids_words_rels[0].2 > 1.85 * word_rel_thresh || (words_len > 1 && ids_words_rels[0].2 > 0.6)
    {
        must_word_idx = ids_words_rels[0].0;
        must_have.insert(ids_words_rels[0].0);
    } else if words_len > 2
        && ids_words_rels[0].2 > word_rel_thresh
        && ids_words_rels[2].2 < word_rel_thresh
    {
        must_word_idx = ids_words_rels[0].0;
        must_have.insert(ids_words_rels[0].0);
    } else if words_len <= 3 && must_have.len() > 0 {
        must_have.insert(ids_words_rels[0].0);
    } else if words_len <= 6 {
        let top_rel_thresh = if words_len > 4 { 0.78 } else { 0.85 };
        if must_have_top_word(&must_have, &ids_words_rels, top_rel_thresh) {
            for (_i, (word_idx, _word, _word_rel)) in ids_words_rels.iter().enumerate() {
                // skip serial numbers, dates
                if numerics.contains(word_idx) || must_have.contains(word_idx) {
                    continue;
                }

                must_word_idx = *word_idx;
                must_have.insert(*word_idx);

                break;
            }
        }
    }

    (must_word_idx, must_have.into_iter().collect())
}

#[inline]
pub fn parse(
    query: &str,
    synonyms_dict: &Option<FnvHashMap<String, String>>,
    toponyms: &Option<fst::Set>,
    stopwords: &FnvHashSet<String>,
    tr_map: &fst::Map,
    mode: ParseMode,
) -> (
    Vec<String>,
    Vec<f32>,
    FnvHashMap<String, Vec<usize>>,
    Vec<String>,
    Vec<f32>,
    Vec<usize>,
    FnvHashMap<usize, String>,
) {
    let mut ngrams_relevs: Vec<f32> = Vec::with_capacity(WORDS_PER_QUERY * 3);
    let mut ngrams: Vec<String> = Vec::with_capacity(WORDS_PER_QUERY * 3);
    let mut ngrams_ids: FnvHashMap<String, Vec<usize>> = FnvHashMap::default();

    let (words, synonyms) = get_norm_query_vec(query, synonyms_dict, mode);
    if words.is_empty() {
        return (
            ngrams,
            ngrams_relevs,
            ngrams_ids,
            words,
            vec![],
            vec![],
            FnvHashMap::default(),
        );
    }

    let words_len = words.len();
    if words_len == 1 {
        update(
            &mut ngrams,
            &mut ngrams_relevs,
            &mut ngrams_ids,
            words[0].clone(),
            1.0,
            vec![0],
        );
        return (
            ngrams,
            ngrams_relevs,
            ngrams_ids,
            words,
            vec![1.0],
            vec![0],
            synonyms,
        );
    }

    let (mut word_idx, stop_idx, words_relevs, must_have, numerics) = index_words(
        &words,
        tr_map,
        stopwords,
        &synonyms,
        &toponyms,
        &synonyms_dict,
    );

    let stop_ngrams = get_stop_ngrams(
        &words,
        &words_relevs,
        &mut word_idx,
        &stop_idx,
        &synonyms,
        mode,
    );

    let stop_ngrams_len = stop_ngrams.len();
    let word_thresh = 1.0 / util::max(2.0, words_len as f32 - 1.0);

    let mut words_vec = words
        .iter()
        .enumerate()
        .zip(words_relevs.iter())
        .map(|(i_w, r)| (i_w.0, i_w.1.to_string(), *r))
        .collect::<Vec<(usize, String, f32)>>();
    words_vec.sort_by(|t1, t2| t1.2.partial_cmp(&t2.2).unwrap_or(Ordering::Less).reverse());

    // bigrams of words with n words in between:
    //  a b c d e f-> [ab, ac, ad, ..., bc, bd, ..., ef]
    let ngram_thresh = 1.8 / words_len as f32;
    for i in 0..stop_ngrams_len {
        if stop_ngrams[i].len() > 1 && stop_ngrams[i].relev > ngram_thresh {
            update(
                &mut ngrams,
                &mut ngrams_relevs,
                &mut ngrams_ids,
                stop_ngrams[i].ngram.clone(),
                stop_ngrams[i].relev,
                stop_ngrams[i].word_indices.clone(),
            );

            if let Some(syn_ngrams) = stop_ngrams.synonyms.get(&i) {
                for syn_ngram in syn_ngrams.iter() {
                    update(
                        &mut ngrams,
                        &mut ngrams_relevs,
                        &mut ngrams_ids,
                        syn_ngram.ngram.clone(),
                        syn_ngram.relev,
                        syn_ngram.word_indices.clone(),
                    );
                }
            }
        }

        for j in i + 1..stop_ngrams_len {
            // if there is a duplicate ngram in the query later, then break here and use that one
            if stop_ngrams[i].ngram == stop_ngrams[j].ngram {
                break;
            }

            let step = j - i - 1;
            let ntr = (1.0 - step as f32 / 100.0) * (stop_ngrams[i].relev + stop_ngrams[j].relev);
            if step < 3 || ntr >= ngram_thresh {
                let ngram = bow2(&stop_ngrams[i].ngram, &stop_ngrams[j].ngram);
                if ngrams_ids.contains_key(&ngram) {
                    continue;
                }
                let mut ngram_ids_vec = stop_ngrams[i].word_indices.clone();
                ngram_ids_vec.extend(stop_ngrams[j].word_indices.clone());

                update(
                    &mut ngrams,
                    &mut ngrams_relevs,
                    &mut ngrams_ids,
                    ngram,
                    ntr,
                    ngram_ids_vec,
                );

                if let Some(syn_ngrams) = stop_ngrams.synonyms.get(&i) {
                    for syn_ngram in syn_ngrams.iter() {
                        let ngram = bow2(&syn_ngram.ngram, &stop_ngrams[j].ngram);
                        if ngrams_ids.contains_key(&ngram) {
                            continue;
                        }
                        let mut ngram_ids_vec = syn_ngram.word_indices.clone();
                        ngram_ids_vec.extend(stop_ngrams[j].word_indices.clone());

                        update(
                            &mut ngrams,
                            &mut ngrams_relevs,
                            &mut ngrams_ids,
                            ngram.clone(),
                            ntr,
                            ngram_ids_vec,
                        );
                    }
                }

                if let Some(syn_ngrams) = stop_ngrams.synonyms.get(&j) {
                    for syn_ngram in syn_ngrams.iter() {
                        let ngram = bow2(&stop_ngrams[i].ngram, &syn_ngram.ngram);
                        if ngrams_ids.contains_key(&ngram) {
                            continue;
                        }
                        let mut ngram_ids_vec = syn_ngram.word_indices.clone();
                        ngram_ids_vec.extend(stop_ngrams[i].word_indices.clone());

                        update(
                            &mut ngrams,
                            &mut ngrams_relevs,
                            &mut ngrams_ids,
                            ngram.clone(),
                            ntr,
                            ngram_ids_vec,
                        );
                    }
                }
            }
        }
    }

    // insert the most relevant word
    if words_len < 4 || words_vec[0].2 > 1.5 * words_vec[1].2 {
        update(
            &mut ngrams,
            &mut ngrams_relevs,
            &mut ngrams_ids,
            words_vec[0].1.clone(),
            words_vec[0].2,
            vec![words_vec[0].0],
        );

        // unigram synonyms
        if mode == ParseMode::Search {
            if let Some(syn) = synonyms.get(&words_vec[0].0) {
                update(
                    &mut ngrams,
                    &mut ngrams_relevs,
                    &mut ngrams_ids,
                    syn.clone(),
                    words_vec[0].2,
                    vec![words_vec[0].0],
                );
            };
        };
    }

    // insert 2nd most relevant word
    if words_vec[1].2 > 0.8 * words_vec[0].2 {
        update(
            &mut ngrams,
            &mut ngrams_relevs,
            &mut ngrams_ids,
            words_vec[1].1.clone(),
            words_vec[1].2,
            vec![words_vec[1].0],
        );

        // unigram synonyms
        if mode == ParseMode::Search {
            if let Some(syn) = synonyms.get(&words_vec[1].0) {
                update(
                    &mut ngrams,
                    &mut ngrams_relevs,
                    &mut ngrams_ids,
                    syn.clone(),
                    words_vec[1].2,
                    vec![words_vec[1].0],
                );
            };
        };
    }

    let (must_word_idx, must_have) =
        find_must_have_words(&words_vec, &must_have, &numerics, words_len, word_thresh);

    if must_word_idx < words_len && words_len < 5 && mode == ParseMode::Search {
        update(
            &mut ngrams,
            &mut ngrams_relevs,
            &mut ngrams_ids,
            words[must_word_idx].clone(),
            words_relevs[must_word_idx],
            vec![must_word_idx],
        );
        if let Some(syn) = synonyms.get(&must_word_idx) {
            update(
                &mut ngrams,
                &mut ngrams_relevs,
                &mut ngrams_ids,
                syn.to_string(),
                words_relevs[must_word_idx],
                vec![must_word_idx],
            );
        };
    }

    // include synonyms for the most important word only
    if words_len > 3 {
        // ngram with 3 most relevant words
        update(
            &mut ngrams,
            &mut ngrams_relevs,
            &mut ngrams_ids,
            bow3(
                &words_vec[0].1.clone(),
                &words_vec[1].1.clone(),
                &words_vec[2].1.clone(),
            ),
            words_vec[0].2 + words_vec[1].2 + words_vec[2].2,
            vec![words_vec[0].0, words_vec[1].0, words_vec[2].0],
        );

        // add (syn, w1, w2)
        if mode == ParseMode::Search {
            if let Some(syn) = synonyms.get(&words_vec[0].0) {
                update(
                    &mut ngrams,
                    &mut ngrams_relevs,
                    &mut ngrams_ids,
                    bow3(&syn, &words_vec[1].1.clone(), &words_vec[2].1.clone()),
                    words_vec[0].2 + words_vec[1].2 + words_vec[2].2,
                    vec![words_vec[0].0, words_vec[1].0, words_vec[2].0],
                );
            };
        };

        if let Some(last) = words_vec.pop() {
            // ngram with the most and the least relevant word
            // if any of the top 2 words is bellow the word_thresh
            if words_vec[0].2 <= word_thresh {
                update(
                    &mut ngrams,
                    &mut ngrams_relevs,
                    &mut ngrams_ids,
                    bow2(&words_vec[0].1.clone(), &last.1.clone()),
                    words_vec[0].2 + last.2,
                    vec![words_vec[0].0, last.0],
                );

                // add (syn_w0, last)
                if mode == ParseMode::Search {
                    if let Some(syn) = synonyms.get(&words_vec[0].0) {
                        update(
                            &mut ngrams,
                            &mut ngrams_relevs,
                            &mut ngrams_ids,
                            bow2(syn, &last.1.clone()),
                            words_vec[0].2 + last.2,
                            vec![words_vec[0].0, last.0],
                        );
                    };
                };
            } else {
                update(
                    &mut ngrams,
                    &mut ngrams_relevs,
                    &mut ngrams_ids,
                    bow2(&words_vec[1].1, &last.1.clone()),
                    words_vec[1].2 + last.2,
                    vec![words_vec[1].0, last.0],
                );
                update(
                    &mut ngrams,
                    &mut ngrams_relevs,
                    &mut ngrams_ids,
                    bow2(&words_vec[1].1, &words_vec[2].1),
                    words_vec[1].2 + words_vec[2].2,
                    vec![words_vec[1].0, words_vec[2].0],
                );

                // add (syn_w1, last), (syn_w1, w2)
                if mode == ParseMode::Search {
                    if let Some(syn) = synonyms.get(&words_vec[1].0) {
                        update(
                            &mut ngrams,
                            &mut ngrams_relevs,
                            &mut ngrams_ids,
                            bow2(syn, &last.1.clone()),
                            words_vec[1].2 + last.2,
                            vec![words_vec[1].0, last.0],
                        );
                        update(
                            &mut ngrams,
                            &mut ngrams_relevs,
                            &mut ngrams_ids,
                            bow2(syn, &words_vec[2].1),
                            words_vec[1].2 + words_vec[2].2,
                            vec![words_vec[1].0, words_vec[2].0],
                        );
                    };
                };
            }
        }
    }

    if words_len >= 3 {
        // 2 most relevant words
        update(
            &mut ngrams,
            &mut ngrams_relevs,
            &mut ngrams_ids,
            bow2(&words_vec[0].1.clone(), &words_vec[1].1.clone()),
            &words_vec[0].2 + &words_vec[1].2,
            vec![words_vec[0].0, words_vec[1].0],
        );

        // add (syn_w0, w1)
        if mode == ParseMode::Search {
            if let Some(syn) = synonyms.get(&words_vec[0].0) {
                update(
                    &mut ngrams,
                    &mut ngrams_relevs,
                    &mut ngrams_ids,
                    bow2(syn, &words_vec[1].1.clone()),
                    words_vec[0].2 + words_vec[1].2,
                    vec![words_vec[0].0, words_vec[1].0],
                );
            };
        };
    }

    if words_len >= 4 {
        // 1st and 3rd
        update(
            &mut ngrams,
            &mut ngrams_relevs,
            &mut ngrams_ids,
            bow2(&words_vec[0].1.clone(), &words_vec[2].1.clone()),
            &words_vec[0].2 + &words_vec[2].2,
            vec![words_vec[0].0, words_vec[2].0],
        );

        // add (syn_0, w2)
        if mode == ParseMode::Search {
            if let Some(syn) = synonyms.get(&words_vec[0].0) {
                update(
                    &mut ngrams,
                    &mut ngrams_relevs,
                    &mut ngrams_ids,
                    bow2(syn, &words_vec[2].1.clone()),
                    words_vec[0].2 + words_vec[2].2,
                    vec![words_vec[0].0, words_vec[2].0],
                );
            };
        };
    }

    (
        ngrams,
        ngrams_relevs,
        ngrams_ids,
        words,
        words_relevs,
        must_have,
        synonyms,
    )
}

#[inline]
pub fn match_queries(
    cand_query: &str,
    words_set: &FnvHashSet<String>,
    cand_synonyms: &FnvHashMap<String, String>,
) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    let cand_words = normalize(cand_query)
        .clone()
        .split(" ")
        .filter(|w| w.len() > 1 || (w.len() == 1 && w.chars().next().unwrap().is_digit(10)))
        .map(|w| w.to_string())
        .collect::<Vec<String>>();

    let mut cand_words_set: FnvHashSet<String> = cand_words.iter().map(|w| w.to_string()).collect();
    let mut match_words = cand_words_set
        .intersection(&words_set)
        .map(|w| w.to_string())
        .collect::<FnvHashSet<String>>();

    for (cand_word, syn) in cand_synonyms {
        if cand_words_set.contains(cand_word) && !match_words.contains(syn) {
            match_words.insert(syn.to_string());
            cand_words_set.remove(cand_word);
        }
    }

    let miss_words = words_set
        .difference(&match_words)
        .map(|w| w.to_string())
        .collect::<Vec<String>>();

    let excess_words = cand_words_set
        .difference(&match_words)
        .map(|w| w.to_string())
        .collect::<Vec<String>>();

    (
        cand_words,
        match_words.into_iter().collect(),
        miss_words,
        excess_words,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use fst::Map;
    use std::path::PathBuf;
    use stopwords;
    use synonyms;
    use toponyms;
    use util::*;

    #[test]
    fn test_suffix_words() {
        let q = "@xel en e x";
        let e = vec!["xelenex"];
        let (words, _) = get_norm_query_vec(q, &None, ParseMode::Search);
        assert_eq!(words, e);
    }

    #[test]
    fn test_u8_find_and_replace() {
        let q = "'Here's@#An ##example!";
        let e = "heres  an   example";
        assert_eq!(normalize(q), e);

        let q = "'Here's@#another one with some question?## and a comma, and (parenthesis)!";
        let e = "heres  another one with some question   and a comma and  parenthesis";
        assert_eq!(normalize(q), e);

        let q = "München Gödel Gießen Bären";
        let e = "muenchen goedel giessen baeren";
        assert_eq!(normalize(q), e);

        let q = "root_type hubspot";
        let e = "root type hubspot";
        assert_eq!(normalize(q), e);
    }

    #[test]
    fn test_separate_digits() {
        let q = "123movies123free";
        let e = "123 movies 123 free";
        assert_eq!(normalize(q), e);

        let q = "ormlite h2";
        let e = "ormlite h2";
        assert_eq!(normalize(q), e);

        let q = "peer2peer";
        let e = "peer2peer";
        assert_eq!(normalize(q), e);

        let q = "peer22peer";
        let e = "peer 22 peer";
        assert_eq!(normalize(q), e);

        let q = "friends s01 e01 stream";
        let e = "friends s01 e01 stream";
        assert_eq!(normalize(q), e);

        let q = "laptop-ersatzteile24";
        let e = "laptop ersatzteile 24";
        assert_eq!(normalize(q), e);

        let q = "ersatzteile24 laptop";
        let e = "ersatzteile 24  laptop";
        assert_eq!(normalize(q), e);

        let q = "neumeyer str. 22 24nürnberg";
        let e = "neumeyer str   22   24 nuernberg";
        assert_eq!(normalize(q), e);

        let q = "neumeyer str. 22-24nürnberg";
        let e = "neumeyer str   22   24 nuernberg";
        assert_eq!(normalize(q), e);
    }

    #[test]
    fn test_get_norm_query_vec() {
        let q = "ruby date and time as string";
        let e = vec!["ruby", "date", "and", "time", "as", "string"];
        let (words, _) = get_norm_query_vec(q, &None, ParseMode::Search);
        assert_eq!(words, e);

        let q = "sim karte defekt t mobile iphone";
        let e_words = vec!["sim", "karte", "defekt", "mobile", "iphone"];
        // TODO fix defektt -> tmobile
        let e_synonyms = vec![(2, "defektt")]
            .into_iter()
            .map(|(i, s)| (i, s.to_string()))
            .collect::<FnvHashMap<usize, String>>();
        let (words, synonyms) = get_norm_query_vec(q, &None, ParseMode::Search);
        assert_eq!(words, e_words);
        assert_eq!(synonyms, e_synonyms);

        let q = "sim karte defekt t mobile iphone";
        let e_words = vec!["sim", "karte", "defekt", "mobile", "iphone"];
        let e_suffix_letters: FnvHashMap<usize, String> = FnvHashMap::default();
        let (words, suffix_letters) = get_norm_query_vec(q, &None, ParseMode::Index);
        assert_eq!(words, e_words);
        assert_eq!(suffix_letters, e_suffix_letters);

        let q = "caddy14 d ersatzteile";
        let e_words = vec!["caddy", "14", "ersatzteile"];
        let e_synonyms = vec![(1, "14d")]
            .into_iter()
            .map(|(i, s)| (i, s.to_string()))
            .collect::<FnvHashMap<usize, String>>();
        let (words, synonyms) = get_norm_query_vec(q, &None, ParseMode::Search);
        assert_eq!(words, e_words);
        assert_eq!(synonyms, e_synonyms);

        let q = "caddy14 d ersatzteile";
        let e_words = vec!["caddy", "14", "ersatzteile"];
        let e_synonyms: FnvHashMap<usize, String> = FnvHashMap::default();
        let (words, synonyms) = get_norm_query_vec(q, &None, ParseMode::Index);
        assert_eq!(words, e_words);
        assert_eq!(synonyms, e_synonyms);

        let q = "r sim 7 free mobile iphone 5";
        let e = vec!["r", "sim", "7", "free", "mobile", "iphone", "5"];
        let (words, _) = get_norm_query_vec(q, &None, ParseMode::Search);
        assert_eq!(words, e);
    }

    fn get_stop_ngrams_test(
        query: &str,
        tr_map: &Map,
        stopwords: &FnvHashSet<String>,
        mode: ParseMode,
    ) -> Vec<String> {
        let (words, _) = get_norm_query_vec(query, &None, mode);

        if words.is_empty() {
            return vec![];
        }

        if words.len() == 1 {
            return words;
        }

        let synonyms: FnvHashMap<usize, String> = FnvHashMap::default();
        let (mut word_idx, stop_idx, rels, _, _) =
            index_words(&words, tr_map, stopwords, &synonyms, &None, &None);

        let stop_ngrams = get_stop_ngrams(&words, &rels, &mut word_idx, &stop_idx, &synonyms, mode);

        stop_ngrams
            .ngrams
            .iter()
            .map(|n| n.ngram.clone())
            .collect::<Vec<String>>()
    }

    #[test]
    fn test_get_stop_ngrams() {
        let stopwords = match stopwords::load("./index/stopwords.txt") {
            Ok(stopwords) => stopwords,
            Err(_) => panic!([
                BYELL,
                "No such file or directory: ",
                ECOL,
                BRED,
                "../index/stopwords.txt",
                ECOL
            ]
            .join("")),
        };

        let tr_map = match Map::from_path("./index/terms_relevance.fst") {
            Ok(tr_map) => tr_map,
            Err(_) => panic!("Failed to load terms rel. map!"),
        };

        let mode = ParseMode::Index;

        let q = "watch the magicians season 4 free 123";
        let e = vec!["watch", "magicians the", "4 season", "free", "123"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "watch the magicians season 4 episode 1 free 123";
        let e = vec![
            "watch",
            "magicians the",
            "4 season",
            "1 episode",
            "free",
            "123",
        ];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "watch the magicians season 4 episode 1 123movies";
        let e = vec![
            "watch",
            "magicians the",
            "4 season",
            "1 episode",
            "123",
            "movies",
        ];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "ormlite callintransaction and h2";
        let e = vec!["ormlite", "and callintransaction", "h2"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "laravel has many order by";
        let e = vec!["laravel", "has many", "by order"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "order by has many laravel";
        let e = vec!["by order", "has many", "laravel"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "the paws of destiny amazon prime";
        let e = vec!["paws the", "destiny of", "amazon", "prime"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "the clash influences";
        let e = vec!["clash the", "influences"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "welche 30 unternehmen sind im dax";
        let e = vec!["30 welche", "unternehmen", "dax im sind"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "if the word is numeric it has to go into bigram";
        let e = vec![
            "if the word",
            "is numeric",
            "has it",
            "go to",
            "bigram into",
        ];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "if the word is 7 it has to go into bigram";
        let e = vec!["if the word", "7 is", "has it", "go to", "bigram into"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "remove all of the spaces in JavaScript file";
        let e = vec!["all remove", "of spaces the", "in javascript", "file"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "allocating memory is not a big deal";
        let e = vec!["allocating", "is memory", "big not", "deal"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "aber keine";
        let e = vec!["aber keine"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        // w/o stopwords
        let q = "hengstenberg evangelische";
        let e = vec!["hengstenberg", "evangelische"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "changing mac os menu bar";
        let e = vec!["changing", "mac", "os", "menu", "bar"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "emacs bind buffer mode key";
        let e = vec!["emacs", "bind", "buffer", "mode", "key"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "sim karte defekt t mobile iphone";
        let e = vec!["sim", "karte", "defekt", "mobile", "iphone"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "disneyland paris ticket download";
        let e = vec!["disneyland", "paris", "ticket", "download"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "friends s01 e01 stream";
        let e = vec!["friends", "s01", "e01", "stream"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "caddy14 d ersatzteile";
        let e = vec!["caddy", "14", "ersatzteile"];
        assert_eq!(get_stop_ngrams_test(q, &tr_map, &stopwords, mode), e);

        let q = "who was the first to invent bicycle";
        let e = vec!["the was who", "first", "invent to", "bicycle"];
        assert_eq!(
            get_stop_ngrams_test(q, &tr_map, &stopwords, ParseMode::Index),
            e
        );
        assert_eq!(
            get_stop_ngrams_test(q, &tr_map, &stopwords, ParseMode::Search),
            e
        );

        let q = "youngest person to walk on the moon";
        let e = vec!["youngest", "person", "to walk", "moon on the"];
        assert_eq!(
            get_stop_ngrams_test(q, &tr_map, &stopwords, ParseMode::Index),
            e
        );
        assert_eq!(
            get_stop_ngrams_test(q, &tr_map, &stopwords, ParseMode::Search),
            e
        );

        let q = "youngest person on the moon";
        let e = vec!["youngest", "person", "moon on the"];
        assert_eq!(
            get_stop_ngrams_test(q, &tr_map, &stopwords, ParseMode::Index),
            e
        );
        assert_eq!(
            get_stop_ngrams_test(q, &tr_map, &stopwords, ParseMode::Search),
            e
        );

        let q = "@x s e l e n a x";
        let e = vec!["xselenax"];
        assert_eq!(
            get_stop_ngrams_test(q, &tr_map, &stopwords, ParseMode::Index),
            e
        );
        assert_eq!(
            get_stop_ngrams_test(q, &tr_map, &stopwords, ParseMode::Search),
            e
        );

        let q = "@xsel e n a x";
        let e = vec!["xselenax"];
        assert_eq!(
            get_stop_ngrams_test(q, &tr_map, &stopwords, ParseMode::Index),
            e
        );
        assert_eq!(
            get_stop_ngrams_test(q, &tr_map, &stopwords, ParseMode::Search),
            e
        );

        let q = "bdv e670";
        let e = vec!["bdv", "e670"];
        assert_eq!(
            get_stop_ngrams_test(q, &tr_map, &stopwords, ParseMode::Index),
            e
        );
        assert_eq!(
            get_stop_ngrams_test(q, &tr_map, &stopwords, ParseMode::Search),
            e
        );
    }

    fn assert_must_have_words_ngrams_ids(
        query: &str,
        synonyms: &Option<FnvHashMap<String, String>>,
        toponyms: &Option<fst::Set>,
        stopwords: &FnvHashSet<String>,
        tr_map: &fst::Map,
        mode: ParseMode,
        e_must_have: Vec<usize>,
        e_words: Vec<&str>,
        e_ngrams_ids: Vec<(&str, Vec<usize>)>,
    ) {
        let (_, _, ngrams_ids, words, _, must_have, _) =
            parse(query, synonyms, &toponyms, &stopwords, &tr_map, mode);
        let e_ngrams_ids = e_ngrams_ids
            .into_iter()
            .map(|(s, v)| (s.to_string(), v))
            .collect::<FnvHashMap<String, Vec<usize>>>();

        assert_eq!(ngrams_ids, e_ngrams_ids);
        assert_eq!(words, e_words);
        assert_eq!(must_have, e_must_have, "query: {}", query);
    }

    #[test]
    fn test_parse() {
        let synonyms = synonyms::load(&PathBuf::from("./index/synonyms.txt"));
        let toponyms = toponyms::load(&PathBuf::from("./index/toponyms.fst"));

        let stopwords = match stopwords::load("./index/stopwords.txt") {
            Ok(stopwords) => stopwords,
            Err(_) => panic!([
                BYELL,
                "No such file or directory: ",
                ECOL,
                BRED,
                "../index/stopwords.txt",
                ECOL
            ]
            .join("")),
        };

        let tr_map = match Map::from_path("./index/terms_relevance.fst") {
            Ok(tr_map) => tr_map,
            Err(_) => panic!("Failed to load terms rel. map!"),
        };

        let q = "list of literature genres txt";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![0, 2],
            vec!["list", "of", "literature", "genres", "txt"],
            vec![
                ("list literature of", vec![0, 1, 2]),
                ("literature of", vec![1, 2]),
                ("genres list", vec![3, 0]),
                ("genres txt", vec![3, 4]),
                ("genres txt list", vec![0, 3, 4]),
                ("genres literature", vec![2, 3]),
                ("list literature", vec![2, 0]),
                ("genres txt literature of", vec![1, 2, 3, 4]),
                ("genres list literature", vec![2, 3, 0]),
                ("genres of", vec![3, 1]),
                ("genres", vec![3]),
            ],
        );

        let q = "friends s01 e01 stream";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![0, 3, 2],
            vec!["friends", "s01", "e01", "stream"],
            vec![
                ("s01 stream", vec![1, 3]),
                ("friends s01", vec![0, 1]),
                ("e01 s01", vec![1, 2]),
                ("friends stream", vec![0, 3]),
                ("e01 friends", vec![0, 2]),
                ("e01 stream", vec![2, 3]),
                ("e01 friends s01", vec![2, 1, 0]),
                ("s01", vec![1]), // fix e02 is missing but it's the top word
            ],
        );

        let q = "emacs bind buffer mode key";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![1, 0],
            vec!["emacs", "bind", "buffer", "mode", "key"],
            vec![
                ("emacs key", vec![0, 4]),
                ("buffer emacs", vec![0, 2]),
                ("bind emacs", vec![0, 1]),
                ("bind buffer", vec![1, 2]),
                ("emacs mode", vec![0, 3]),
                ("buffer mode", vec![2, 3]),
                ("buffer key", vec![2, 4]),
                ("key mode", vec![3, 4]),
                ("bind buffer emacs", vec![0, 2, 1]),
                ("bind mode", vec![1, 3]),
                ("bind key", vec![1, 4]),
                ("emacs", vec![0]),
            ],
        );

        let q = "disneyland paris ticket download";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![1, 0],
            vec!["disneyland", "paris", "ticket", "download"],
            vec![
                ("disneyland paris", vec![0, 1]),
                ("paris ticket", vec![1, 2]),
                ("download ticket", vec![2, 3]),
                ("disneyland download", vec![0, 3]),
                ("disneyland", vec![0]),
                ("disneyland paris ticket", vec![0, 1, 2]),
                ("disneyland ticket", vec![0, 2]),
                ("download paris", vec![1, 3]),
            ],
        );

        let q = "cisco 4500e power supply configuration manager";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![1, 0],
            vec![
                "cisco",
                "4500",
                "power",
                "supply",
                "configuration",
                "manager",
            ],
            vec![
                ("power supply", vec![2, 3]),
                ("manager power", vec![2, 5]),
                ("4500 manager", vec![1, 5]),
                ("manager supply", vec![3, 5]),
                ("configuration power", vec![2, 4]),
                ("configuration supply", vec![3, 4]),
                ("configuration manager", vec![4, 5]),
                ("cisco configuration", vec![0, 4]),
                ("cisco manager", vec![0, 5]),
                ("cisco power", vec![0, 2]),
                ("cisco supply", vec![0, 3]),
                ("4500 cisco supply", vec![0, 1, 3]),
                ("4500 configuration", vec![1, 4]),
                ("4500 cisco", vec![0, 1]),
                ("4500 supply", vec![1, 3]),
                ("4500 power", vec![1, 2]),
                ("4500", vec![1]),
            ],
        );

        let q = "tuhh thesis scholarship";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![0],
            vec!["tuhh", "thesis", "scholarship"],
            vec![
                ("thesis tuhh", vec![0, 1]),
                ("scholarship thesis", vec![1, 2]),
                ("scholarship tuhh", vec![0, 2]),
                ("tuhh", vec![0]),
            ],
        );

        let q = "welche 30 unternehmen sind im dax";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![1, 5],
            vec!["welche", "30", "unternehmen", "sind", "im", "dax"],
            vec![
                ("unternehmen welche", vec![2, 0]),
                ("30 welche unternehmen", vec![0, 1, 2]),
                ("30 dax", vec![5, 1]),
                ("30 welche dax im sind", vec![0, 1, 3, 4, 5]),
                ("dax im sind", vec![3, 4, 5]),
                ("dax unternehmen", vec![5, 2]),
                ("30 unternehmen", vec![2, 1]),
                ("30 dax unternehmen", vec![5, 2, 1]),
                ("dax im sind unternehmen", vec![2, 3, 4, 5]),
                ("dax", vec![5]),
            ],
        );

        let q = "nidda in alter zeit";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![0],
            vec!["nidda", "in", "alter", "zeit"],
            vec![
                ("alter zeit", vec![3, 2]),
                ("in zeit", vec![3, 1]),
                ("alter nidda", vec![0, 2]),
                ("nidda zeit", vec![0, 3]),
                ("alter in nidda", vec![0, 1, 2]),
                ("nidda", vec![0]),
                ("alter nidda zeit", vec![0, 3, 2]),
                ("alter in zeit", vec![1, 2, 3]),
            ],
        );

        let q = "fsck inode has imagic flag set";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![4],
            vec!["fsck", "inode", "has", "imagic", "flag", "set"],
            vec![
                ("fsck inode", vec![0, 1]),
                ("flag fsck", vec![0, 4]),
                // ("fsck imagic set", vec![2, 3, 5]),
                ("has imagic", vec![2, 3]),
                ("flag has imagic", vec![2, 3, 4]),
                ("flag set", vec![4, 5]),
                ("fsck has imagic", vec![0, 2, 3]),
                ("fsck imagic inode", vec![3, 1, 0]),
                ("has imagic set", vec![2, 3, 5]),
                ("has imagic inode", vec![1, 2, 3]),
                ("has inode", vec![1, 2]),
                ("imagic inode", vec![3, 1]),
                ("flag inode", vec![1, 4]),
                ("inode set", vec![1, 5]),
                ("fsck imagic", vec![3, 0]),
            ],
        );

        let q = "python programming to iota";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![3],
            vec!["python", "programming", "to", "iota"],
            vec![
                ("iota to programming", vec![1, 2, 3]),
                ("python to", vec![0, 2]),
                ("iota to python", vec![0, 2, 3]),
                ("iota to", vec![2, 3]),
                ("iota python", vec![3, 0]),
                ("iota programming python", vec![3, 0, 1]),
                ("programming python", vec![0, 1]),
                ("iota programming", vec![3, 1]),
                ("iota", vec![3]),
            ],
        );

        let q = "dinkel vollkorn toasties rezept";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![0, 2],
            vec!["dinkel", "vollkorn", "toasties", "rezept"],
            vec![
                ("toasties vollkorn", vec![1, 2]),
                ("rezept toasties", vec![2, 3]),
                ("rezept vollkorn", vec![1, 3]),
                ("dinkel toasties vollkorn", vec![2, 0, 1]),
                ("dinkel rezept", vec![0, 3]),
                ("dinkel vollkorn", vec![0, 1]),
                ("dinkel toasties", vec![0, 2]),
            ],
        );

        let q = "laravel has many order by";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![0, 2],
            vec!["laravel", "has", "many", "order", "by"],
            // fix form 'has many' instead of 'has laravel'
            vec![
                ("by order has many", vec![1, 2, 3, 4]),
                ("has many laravel", vec![0, 1, 2]),
                ("by order laravel", vec![0, 3, 4]),
                ("laravel order", vec![0, 3]),
                ("many order", vec![3, 2]),
                ("laravel many order", vec![0, 3, 2]),
                ("laravel many", vec![0, 2]),
                ("laravel", vec![0]),
                ("has order", vec![3, 1]),
            ],
        );

        let q = "samsung tv skype 2017";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![0, 3, 2],
            vec!["samsung", "tv", "skype", "2017"],
            vec![
                ("samsung tv", vec![0, 1]),
                ("2017 samsung", vec![0, 3]),
                ("2017 tv", vec![1, 3]),
                ("2017 skype", vec![2, 3]),
                ("samsung skype tv", vec![2, 0, 1]),
                ("samsung skype", vec![0, 2]),
                ("skype tv", vec![1, 2]),
            ],
        );

        let q = "positionierte stl datei exportieren catia";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![4],
            vec!["positionierte", "stl", "datei", "exportieren", "catia"],
            vec![
                ("positionierte stl", vec![0, 1]),
                ("catia positionierte", vec![0, 4]),
                ("exportieren stl", vec![1, 3]),
                ("datei exportieren", vec![2, 3]),
                ("catia exportieren", vec![3, 4]),
                ("datei stl", vec![1, 2]),
                ("datei positionierte", vec![0, 2]),
                ("catia positionierte stl", vec![4, 0, 1]),
                ("exportieren positionierte", vec![0, 3]),
                ("catia stl", vec![1, 4]),
                ("catia datei", vec![2, 4]),
            ],
        );

        let q = "caddy14 d ersatzteile";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![1, 0],
            vec!["caddy", "14", "ersatzteile"],
            vec![
                ("caddy ersatzteile", vec![0, 2]),
                ("14 ersatzteile", vec![1, 2]),
                ("14 caddy", vec![0, 1]),
                ("caddy", vec![0]),
            ],
        );

        // search mode!
        let q = "caddy14 d ersatzteile";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Search,
            vec![1],
            vec!["caddy", "14", "ersatzteile"],
            vec![
                ("caddy", vec![0]),
                ("14d caddy", vec![1, 0]),
                ("14d", vec![1]),
                ("14", vec![1]),
                ("14 caddy", vec![0, 1]),
                ("caddy ersatzteile", vec![0, 2]),
                ("14d ersatzteile", vec![1, 2]),
                ("14 ersatzteile", vec![1, 2]),
            ],
        );

        // search mode!
        let q = "pokemon best vs rock";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Search,
            vec![0, 3],
            vec!["pokemon", "best", "vs", "rock"],
            vec![
                ("best rock", vec![1, 3]),
                ("against rock", vec![2, 3]),
                ("best pokemon", vec![0, 1]),
                ("best vs", vec![1, 2]),
                ("against best", vec![2, 1]),
                ("rock", vec![3]),
                ("pokemon rock vs", vec![0, 3, 2]),
                ("rock vs", vec![2, 3]),
                ("against pokemon", vec![2, 0]),
                ("pokemon rock", vec![0, 3]),
                ("pokemon vs", vec![0, 2]),
                ("pokemon", vec![0]),
            ],
        );

        let q = "friends s01 e01 stream";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Search,
            vec![0, 3, 2],
            vec!["friends", "s01", "e01", "stream"],
            vec![
                ("friends streaming", vec![3, 0]),
                ("s01 streaming", vec![3, 1]),
                ("e01 streaming", vec![3, 2]),
                ("s01 stream", vec![1, 3]),
                ("friends", vec![0]),
                ("friends s01", vec![0, 1]),
                ("e01 s01", vec![1, 2]),
                ("friends stream", vec![0, 3]),
                ("e01 friends", vec![0, 2]),
                ("e01 stream", vec![2, 3]),
                ("e01 friends s01", vec![2, 1, 0]),
                ("s01", vec![1]), // fix e02 is missing but it's the top word
            ],
        );

        let q = "calypso k5177"; // fix k5177==k5117, calypso is not included in ngrams,
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Search,
            vec![1, 0],
            vec!["calypso", "k5177"],
            vec![("calypso k5177", vec![0, 1]), ("k5177", vec![1])],
        );

        let q = "kenzan flowers size";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Search,
            vec![1, 0],
            vec!["kenzan", "flowers", "size"],
            vec![
                ("kenzan size", vec![0, 2]),
                ("flowers size", vec![1, 2]),
                ("kenzan", vec![0]),
                ("flowers kenzan", vec![0, 1]),
            ],
        );
        // assert equal outcomes for different parsing modes
        let (_, _, s_ngrams_ids, s_words, _, s_must_have, _) = parse(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
        );
        let (_, _, i_ngrams_ids, i_words, _, i_must_have, _) = parse(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Search,
        );
        assert_eq!(s_ngrams_ids, i_ngrams_ids);
        assert_eq!(s_words, i_words);
        assert_eq!(s_must_have, i_must_have, "query: {}", q);

        let q = "what size kenzan";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Search,
            vec![2],
            vec!["what", "size", "kenzan"],
            vec![
                ("kenzan size what", vec![0, 1, 2]),
                ("kenzan size", vec![2, 1]),
                ("kenzan", vec![2]),
            ],
        );
        // assert equal outcomes for different parsing modes
        let (_, _, s_ngrams_ids, s_words, _, s_must_have, _) =
            parse(q, &None, &None, &stopwords, &tr_map, ParseMode::Index);
        let (_, _, i_ngrams_ids, i_words, _, i_must_have, _) =
            parse(q, &None, &None, &stopwords, &tr_map, ParseMode::Search);
        assert_eq!(s_ngrams_ids, i_ngrams_ids, "query: {}", q);
        assert_eq!(s_words, i_words);
        assert_eq!(s_must_have, i_must_have, "query: {}", q);

        let q = "ormlite callintransaction and h2";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Search,
            vec![0, 3],
            vec!["ormlite", "callintransaction", "and", "h2"],
            vec![
                ("and callintransaction h2", vec![1, 2, 3]),
                ("and callintransaction ormlite", vec![0, 1, 2]),
                ("callintransaction h2", vec![1, 3]),
                ("callintransaction ormlite", vec![0, 1]),
                ("and callintransaction", vec![1, 2]),
                ("h2 ormlite", vec![0, 3]),
                ("ormlite", vec![0]),
                ("callintransaction h2 ormlite", vec![0, 1, 3]),
            ],
        );
        // assert (not) equal outcomes for different parsing modes [ormlite missing on indexing part]
        let (_, _, s_ngrams_ids, s_words, _, s_must_have, _) =
            parse(q, &None, &None, &stopwords, &tr_map, ParseMode::Search);
        let (_, _, i_ngrams_ids, i_words, _, i_must_have, _) =
            parse(q, &None, &None, &stopwords, &tr_map, ParseMode::Index);
        assert_ne!(s_ngrams_ids, i_ngrams_ids, "query: {}", q);
        assert_eq!(s_words, i_words);
        assert_eq!(s_must_have, i_must_have, "query: {}", q);

        let q = "who was the first to invent bicycle";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Index,
            vec![6],
            vec!["who", "was", "the", "first", "to", "invent", "bicycle"],
            vec![
                ("first invent", vec![5, 3]),
                ("bicycle first invent", vec![6, 5, 3]),
                ("bicycle invent to", vec![4, 5, 6]),
                ("first the was who", vec![0, 1, 2, 3]),
                ("invent to the was who", vec![0, 1, 2, 4, 5]),
                ("first invent to", vec![3, 4, 5]),
                ("invent to", vec![4, 5]),
                ("invent", vec![5]),
                ("bicycle first", vec![3, 6]),
                ("bicycle the was who", vec![0, 1, 2, 6]),
                ("bicycle invent", vec![6, 5]),
            ],
        );

        let q = "who was the first to invent bicycle";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Search, // include synonyms bicycle -> bike
            vec![6],
            vec!["who", "was", "the", "first", "to", "invent", "bicycle"],
            vec![
                ("first invent", vec![5, 3]),
                ("bicycle first invent", vec![6, 5, 3]),
                ("bike first invent", vec![6, 5, 3]),
                ("bicycle invent to", vec![4, 5, 6]),
                ("bike invent to", vec![6, 4, 5]),
                ("first the was who", vec![0, 1, 2, 3]),
                ("invent to the was who", vec![0, 1, 2, 4, 5]),
                ("first invent to", vec![3, 4, 5]),
                ("invent to", vec![4, 5]),
                ("invent", vec![5]),
                ("bicycle first", vec![3, 6]),
                ("bike first", vec![6, 3]),
                ("bicycle the was who", vec![0, 1, 2, 6]),
                ("bike the was who", vec![6, 0, 1, 2]),
                ("bicycle invent", vec![6, 5]),
                ("bike invent", vec![6, 5]),
            ],
        );

        let q = "youngest person to walk on the moon";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Search,
            vec![6],
            vec!["youngest", "person", "to", "walk", "on", "the", "moon"],
            vec![
                ("moon on the", vec![4, 5, 6]),
                ("moon walk youngest", vec![6, 3, 0]),
                ("moon on the to walk", vec![2, 3, 4, 5, 6]),
                ("person youngest", vec![0, 1]),
                ("to walk", vec![2, 3]),
                ("to walk youngest", vec![0, 2, 3]),
                ("moon on the person", vec![1, 4, 5, 6]),
                ("moon on the youngest", vec![0, 4, 5, 6]),
                ("person to walk", vec![1, 2, 3]),
                ("on walk", vec![3, 4]),
                ("walk youngest", vec![3, 0]),
                ("moon walk", vec![6, 3]),
                ("moon youngest", vec![6, 0]),
            ],
        );

        let q = "youngest person on the moon";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Search,
            vec![4],
            vec!["youngest", "person", "on", "the", "moon"],
            vec![
                ("moon person youngest", vec![4, 0, 1]),
                ("moon on the", vec![2, 3, 4]),
                ("moon youngest", vec![4, 0]),
                ("person youngest", vec![0, 1]),
                ("on youngest", vec![0, 2]),
                ("moon on the person", vec![1, 2, 3, 4]),
                ("moon on the youngest", vec![0, 2, 3, 4]),
                ("moon person", vec![4, 1]),
            ],
        );

        let q = "blutdruck 95 zu 56 puls 113";
        assert_must_have_words_ngrams_ids(
            q,
            &synonyms,
            &toponyms,
            &stopwords,
            &tr_map,
            ParseMode::Search,
            vec![5, 4, 0],
            vec!["blutdruck", "95", "zu", "56", "puls", "113"],
            vec![
                ("56 zu 95", vec![1, 2, 3]),
                ("95 blutdruck", vec![0, 1]),
                ("56 zu blutdruck", vec![0, 2, 3]),
                ("113 blutdruck", vec![0, 5]),
                ("95 puls", vec![1, 4]),
                ("blutdruck puls", vec![0, 4]),
                ("56 zu puls", vec![2, 3, 4]),
                ("113 puls", vec![4, 5]),
                ("113", vec![5]),
                ("113 blutdruck puls", vec![0, 5, 4]),
                ("113 95", vec![1, 5]),
                ("113 zu", vec![5, 2]),
                ("113 56 zu", vec![2, 3, 5]),
            ],
        );
    }

    fn assert_match_miss_excess(
        q_cand: &str,
        org_q: &FnvHashSet<String>,
        cand_syns: &FnvHashMap<String, String>,
        e_cand: Vec<&str>,
        e_match: Vec<&str>,
        e_miss: Vec<&str>,
        e_excess: Vec<&str>,
    ) {
        let (cand_words, match_words, miss_words, excess_words) =
            match_queries(q_cand, &org_q, &cand_syns);
        assert_eq!(cand_words, e_cand);
        assert_eq!(match_words, e_match);
        assert_eq!(miss_words, e_miss);
        assert_eq!(excess_words, e_excess);
    }

    #[test]
    fn test_match_queries() {
        let org_q: FnvHashSet<String> = vec!["several", "million"]
            .into_iter()
            .map(|w| w.to_string())
            .collect();

        let cand_syns: FnvHashMap<String, String> = vec![("millions", "million")]
            .into_iter()
            .map(|(w1, w2)| (w1.to_string(), w2.to_string()))
            .collect();

        let q_cand = "several millions";
        let (e_cand, e_match) = (vec!["several", "millions"], vec!["several", "million"]);
        let (e_miss, e_excess): (Vec<&str>, Vec<&str>) = (vec![], vec![]);
        assert_match_miss_excess(
            q_cand, &org_q, &cand_syns, e_cand, e_match, e_miss, e_excess,
        );

        let q_cand = "2 millions";
        let (e_cand, e_match, e_miss, e_excess) = (
            vec!["2", "millions"],
            vec!["million"],
            vec!["several"],
            vec!["2"],
        );
        assert_match_miss_excess(
            q_cand, &org_q, &cand_syns, e_cand, e_match, e_miss, e_excess,
        );

        let q_cand = "several million or millions";
        let (e_cand, e_match, e_excess) = (
            vec!["several", "million", "or", "millions"],
            vec!["several", "million"],
            vec!["or", "millions"],
        );
        let e_miss: Vec<&str> = vec![];
        assert_match_miss_excess(
            q_cand, &org_q, &cand_syns, e_cand, e_match, e_miss, e_excess,
        );

        let q_cand = "several million or several millions";
        let (e_cand, e_match, e_excess) = (
            vec!["several", "million", "or", "several", "millions"],
            vec!["several", "million"],
            vec!["or", "millions"],
        );
        let e_miss: Vec<&str> = vec![];
        assert_match_miss_excess(
            q_cand, &org_q, &cand_syns, e_cand, e_match, e_miss, e_excess,
        );

        let q_cand = "several million vs several millions";
        let (e_cand, e_match, e_excess) = (
            vec!["several", "million", "vs", "several", "millions"],
            vec!["several", "million"],
            vec!["millions", "vs"],
        );
        let e_miss: Vec<&str> = vec![];
        assert_match_miss_excess(
            q_cand, &org_q, &cand_syns, e_cand, e_match, e_miss, e_excess,
        );
    }
}
