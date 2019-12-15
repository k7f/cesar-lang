use std::{convert::TryInto, error::Error};
use log::Level::Debug;
use aces::{ContextHandle, PartialContent, CompilableAsContent};
use crate::{CesInstance, Node, NodeList, BinOp, polynomial::Polynomial, AscesisError};

pub(crate) type RexID = usize;

#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub(crate) struct RexTree {
    ids: Vec<RexID>,
}

impl RexTree {
    pub(crate) fn as_slice(&self) -> &[RexID] {
        self.ids.as_slice()
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rex {
    pub(crate) kinds: Vec<RexKind>,
}

impl Rex {
    pub(crate) fn with_more(self, rexlist: Vec<(Option<BinOp>, Rex)>) -> Self {
        if rexlist.is_empty() {
            return self
        }

        let plusless = rexlist.iter().all(|(op, _)| op.is_none());

        if plusless {
            let mut kinds = vec![RexKind::Product(RexTree::default())];

            let mut ids = vec![1];
            let mut offset = kinds.append_with_offset(self.kinds, 1);

            for (_, rex) in rexlist.into_iter() {
                ids.push(offset);
                offset = kinds.append_with_offset(rex.kinds, offset);
            }

            kinds[0] = RexKind::Product(RexTree { ids });

            Rex { kinds }
        } else {
            // this is used for pruning single-factor products
            let followed_by_product: Vec<bool> =
                rexlist.iter().map(|(op, _)| op.is_none()).collect();
            let mut followed_by_product = followed_by_product.into_iter();

            let mut kinds = vec![RexKind::Sum(RexTree::default())];

            let mut sum_ids = Vec::new();
            let mut product_ids = Vec::new();
            let mut anchor = 1; // index in `kinds` of next addend
            let mut offset = 1; // index in `kinds` of next factor

            if followed_by_product.next().unwrap() {
                kinds.push(RexKind::Product(RexTree::default()));
                offset += 1;
                // `offset` points to first factor of first addend, i.e. to the `self`
                product_ids.push(offset);
            }

            offset = kinds.append_with_offset(self.kinds, offset);
            // `offset` points to expected second factor of first addend or to a second addend

            for (op, rex) in rexlist.into_iter() {
                let is_followed_by_product = followed_by_product.next().unwrap_or(false);

                if let Some(op) = op {
                    if op == BinOp::Add {
                        if !product_ids.is_empty() {
                            if let RexKind::Product(tree) = &mut kinds[anchor] {
                                tree.ids.append(&mut product_ids);
                            } else {
                                panic!()
                            }
                        }

                        sum_ids.push(anchor);
                        anchor = offset;

                        if is_followed_by_product {
                            kinds.push(RexKind::Product(RexTree::default()));
                            offset += 1;
                            product_ids.push(offset);
                        }

                        offset = kinds.append_with_offset(rex.kinds, offset);
                    } else {
                        panic!()
                    }
                } else {
                    product_ids.push(offset);
                    offset = kinds.append_with_offset(rex.kinds, offset);
                }
            }

            if !product_ids.is_empty() {
                kinds[anchor] = RexKind::Product(RexTree { ids: product_ids });
            }
            sum_ids.push(anchor);
            kinds[0] = RexKind::Sum(RexTree { ids: sum_ids });

            Rex { kinds }
        }
    }

    /// Returns a copy of this `Rex` converted to the normal form.
    // FIXME the result of FIT transformation should be further
    // simplified.
    pub fn fit_clone(&self) -> Self {
        let mut new_kinds = Vec::new();
        let mut id_map = Vec::new();

        for old_kind in self.kinds.iter() {
            id_map.push(new_kinds.len());

            if let RexKind::Fat(far) = old_kind {
                let tars: Vec<ThinArrowRule> = far.into();
                let ids: Vec<RexID> = std::iter::repeat(0).take(tars.len()).collect();

                new_kinds.push(RexKind::Sum(RexTree { ids }));
                new_kinds.extend(tars.into_iter().map(RexKind::Thin));
            } else {
                new_kinds.push(old_kind.clone());
            }
        }

        for (mut ndx, new_kind) in new_kinds.iter_mut().enumerate() {
            match new_kind {
                RexKind::Product(tree) | RexKind::Sum(tree) => {
                    if let Some(first) = tree.ids.first() {
                        if *first > 0 {
                            for id in tree.ids.iter_mut() {
                                assert!(*id > 0);
                                *id = id_map[*id];
                            }
                        } else {
                            for id in tree.ids.iter_mut() {
                                assert_eq!(*id, 0);
                                ndx += 1;
                                *id = ndx;
                            }
                        }
                    } else {
                        panic!()
                    }
                }
                _ => {}
            }
        }

        Rex { kinds: new_kinds }
    }
}

impl CompilableAsContent for Rex {
    fn check_dependencies(&self, ctx: &ContextHandle) -> Option<String> {
        let ctx = ctx.lock().unwrap();

        for kind in self.kinds.iter() {
            if let RexKind::Instance(instance) = kind {
                if !ctx.has_content(&instance.name) {
                    return Some((*instance.name).clone())
                }
            }
        }

        None
    }

    fn get_compiled_content(&self, ctx: &ContextHandle) -> Result<PartialContent, Box<dyn Error>> {
        let rex = self.fit_clone();

        if rex.kinds.is_empty() {
            return Ok(PartialContent::new(ctx))
        }

        let mut merged_content = vec![None; rex.kinds.len()];
        let mut parent_pos = vec![0; rex.kinds.len()];

        for (pos, kind) in rex.kinds.iter().enumerate() {
            match kind {
                RexKind::Product(ast) | RexKind::Sum(ast) => {
                    merged_content[pos] = Some(PartialContent::new(ctx));

                    debug!("Rex compile node {} -> {:?}", pos, kind);
                    for &i in ast.as_slice() {
                        if i > pos {
                            parent_pos[i] = pos;
                        } else {
                            return Err(AscesisError::InvalidAST.into())
                        }
                    }
                }
                _ => {}
            }
        }

        for pos in (0..rex.kinds.len()).rev() {
            let content = match &rex.kinds[pos] {
                RexKind::Thin(tar) => tar.get_compiled_content(ctx)?,
                RexKind::Fat(_) => return Err(AscesisError::FatLeak.into()),
                RexKind::Instance(instance) => {
                    // FIXME
                    println!("--> in rex, {}", instance.name);
                    let ctx = ctx.lock().unwrap();

                    if let Some(content) = ctx.get_content(&instance.name) {
                        content.clone()
                    } else {
                        return Err(
                            AscesisError::UnexpectedDependency((*instance.name).clone()).into()
                        )
                    }
                }
                RexKind::Product(_) | RexKind::Sum(_) => {
                    if let Some(content) = merged_content[pos].take() {
                        content
                    } else {
                        return Err(AscesisError::InvalidAST.into())
                    }
                }
            };

            if pos > 0 {
                let parent = parent_pos[pos];

                if let Some(parent_content) = merged_content[parent].as_mut() {
                    match &rex.kinds[parent] {
                        RexKind::Product(_) => {
                            *parent_content *= content;
                        }
                        RexKind::Sum(_) => {
                            *parent_content += content;
                        }
                        _ => return Err(AscesisError::InvalidAST.into()),
                    }
                } else {
                    return Err(AscesisError::InvalidAST.into())
                }
            } else {
                return Ok(content)
            }
        }

        unreachable!()
    }
}

impl From<ThinArrowRule> for Rex {
    fn from(rule: ThinArrowRule) -> Self {
        Rex { kinds: vec![RexKind::Thin(rule)] }
    }
}

impl From<FatArrowRule> for Rex {
    fn from(rule: FatArrowRule) -> Self {
        Rex { kinds: vec![RexKind::Fat(rule)] }
    }
}

impl From<CesInstance> for Rex {
    fn from(instance: CesInstance) -> Self {
        Rex { kinds: vec![RexKind::Instance(instance)] }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum RexKind {
    Thin(ThinArrowRule),
    Fat(FatArrowRule),
    Instance(CesInstance),
    Product(RexTree),
    Sum(RexTree),
}

trait AppendWithOffset {
    fn append_with_offset(&mut self, source: Self, offset: usize) -> usize;
}

impl AppendWithOffset for Vec<RexKind> {
    fn append_with_offset(&mut self, source: Self, offset: usize) -> usize {
        let result = offset + source.len();

        self.extend(source.into_iter().map(|mut kind| match kind {
            RexKind::Product(ref mut tree) | RexKind::Sum(ref mut tree) => {
                tree.ids.iter_mut().for_each(|id| *id += offset);
                kind
            }
            _ => kind,
        }));

        result
    }
}

#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub struct ThinArrowRule {
    nodes:  NodeList,
    cause:  Polynomial,
    effect: Polynomial,
}

impl ThinArrowRule {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    pub(crate) fn with_nodes(mut self, nodes: Polynomial) -> Result<Self, Box<dyn Error>> {
        self.nodes = nodes.try_into()?;
        Ok(self)
    }

    pub(crate) fn with_cause(mut self, cause: Polynomial) -> Self {
        self.cause = cause;
        self
    }

    pub(crate) fn with_effect(mut self, effect: Polynomial) -> Self {
        self.effect = effect;
        self
    }

    pub fn get_nodes(&self) -> &[Node] {
        &self.nodes.nodes
    }
}

impl CompilableAsContent for ThinArrowRule {
    fn get_compiled_content(&self, ctx: &ContextHandle) -> Result<PartialContent, Box<dyn Error>> {
        let mut content = PartialContent::new(ctx);

        let cause = self.cause.compile_as_vec(ctx);
        let effect = self.effect.compile_as_vec(ctx);

        let mut debug_mess = if log_enabled!(Debug) {
            if cause.is_empty() {
                format!("E{:?} @ {{", effect)
            } else if effect.is_empty() {
                format!("C{:?} @ {{", cause)
            } else {
                format!("C{:?} E{:?} @ {{", cause, effect)
            }
        } else {
            String::new()
        };

        for node in self.get_nodes() {
            let id = {
                let mut ctx = ctx.lock().unwrap();
                ctx.share_node_name(node)
            };

            if log_enabled!(Debug) {
                debug_mess.push_str(&format!(" {:?}:{:?}", node, id));
            }

            if !cause.is_empty() {
                content.add_to_causes(id, &cause);
            }

            if !effect.is_empty() {
                content.add_to_effects(id, &effect);
            }
        }

        debug!("TAR compile {} }}", debug_mess);

        Ok(content)
    }
}

#[derive(Clone, PartialEq, Eq, Default, Debug)]
struct FatArrow {
    cause:  Polynomial,
    effect: Polynomial,
}

#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub struct FatArrowRule {
    parts: Vec<FatArrow>,
}

impl FatArrowRule {
    pub(crate) fn from_parts(head: Polynomial, tail: Vec<(BinOp, Polynomial)>) -> Self {
        assert!(!tail.is_empty(), "Single-polynomial fat arrow rule");

        let mut far = Self::default();
        let mut prev = head;

        for (op, poly) in tail.into_iter() {
            match op {
                BinOp::FatTx => {
                    far.parts.push(FatArrow { cause: prev, effect: poly.clone() });
                }
                BinOp::FatRx => {
                    far.parts.push(FatArrow { cause: poly.clone(), effect: prev });
                }
                BinOp::FatDx => {
                    far.parts.push(FatArrow { cause: prev.clone(), effect: poly.clone() });
                    far.parts.push(FatArrow { cause: poly.clone(), effect: prev });
                }
                _ => panic!("Operator not allowed in a fat arrow rule: '{}'.", op),
            }
            prev = poly;
        }
        far
    }
}

impl From<FatArrowRule> for Vec<ThinArrowRule> {
    fn from(far: FatArrowRule) -> Self {
        // FIXME specialize, cloning less than in the borrowed version.
        Vec::from(&far)
    }
}

impl From<&FatArrowRule> for Vec<ThinArrowRule> {
    fn from(far: &FatArrowRule) -> Self {
        // FIT (fat-into-thin) transformation steps.

        // 0. Fat arrow rule with more than two polynomials is first
        // transformed into a sum of two-polynomial fat arrow rules,
        // represented here by parts of `FatArrowRule` type.

        // Step 0. is done in FatArrowRule::from_parts().

        // 1. Each two-polynomial (part of a) fat arrow rule is
        // replaced with a sum of two thin arrow rules, one
        // effect-only, another cause-only.

        let mut tx_tars = Vec::new();
        let mut rx_tars = Vec::new();

        for part in far.parts.iter() {
            let sources = part.cause.flattened_clone();
            let sinks = part.effect.flattened_clone();

            tx_tars.push(
                ThinArrowRule::new().with_nodes(sources).unwrap().with_effect(part.effect.clone()),
            );
            rx_tars.push(
                ThinArrowRule::new().with_nodes(sinks).unwrap().with_cause(part.cause.clone()),
            );
        }

        loop {
            let mut at_fixpoint = true;

            // 2. The resulting rule expression is simplified by
            // integrating effect-only rules having a common node list and
            // doing the same with cause-only rules.

            let mut tx_tars_2: Vec<ThinArrowRule> = Vec::new();
            let mut rx_tars_2: Vec<ThinArrowRule> = Vec::new();

            'outer_tx_2: for mut tar_1 in tx_tars {
                for tar_2 in tx_tars_2.iter_mut() {
                    if tar_2.nodes == tar_1.nodes {
                        tar_2.effect.add_assign(&mut tar_1.effect);

                        at_fixpoint = false;
                        continue 'outer_tx_2
                    }
                }
                tx_tars_2.push(tar_1);
            }

            'outer_rx_2: for mut tar_1 in rx_tars {
                for tar_2 in rx_tars_2.iter_mut() {
                    if tar_2.nodes == tar_1.nodes {
                        tar_2.cause.add_assign(&mut tar_1.cause);

                        at_fixpoint = false;
                        continue 'outer_rx_2
                    }
                }
                rx_tars_2.push(tar_1);
            }

            // 3. Rule expression is further simplified by merging
            // node lists which point to the same effect polynomials,
            // and merging node lists pointed to by the same cause
            // polynomials.

            let mut tx_tars_3: Vec<ThinArrowRule> = Vec::new();
            let mut rx_tars_3: Vec<ThinArrowRule> = Vec::new();

            'outer_tx_3: for mut tar_2 in tx_tars_2 {
                for tar_3 in tx_tars_3.iter_mut() {
                    if tar_3.effect == tar_2.effect {
                        tar_3.nodes.add_assign(&mut tar_2.nodes);

                        at_fixpoint = false;
                        continue 'outer_tx_3
                    }
                }
                tx_tars_3.push(tar_2);
            }

            'outer_rx_3: for mut tar_2 in rx_tars_2 {
                for tar_3 in rx_tars_3.iter_mut() {
                    if tar_3.cause == tar_2.cause {
                        tar_3.nodes.add_assign(&mut tar_2.nodes);

                        at_fixpoint = false;
                        continue 'outer_rx_3
                    }
                }
                rx_tars_3.push(tar_2);
            }

            // The result is a sum of single-polynomial thin arrow rules.

            tx_tars = tx_tars_3;
            rx_tars = rx_tars_3;

            // Steps 2. and 3. are repeated, until a fixed point is
            // reached.

            if at_fixpoint {
                break
            }
        }

        // 4. Any pair of rules with the same node list is combined
        // into a two-polynomial rule.

        'outer_4: for rx_tar in rx_tars {
            for tx_tar in tx_tars.iter_mut() {
                if rx_tar.nodes == tx_tar.nodes {
                    tx_tar.cause = rx_tar.cause;
                    continue 'outer_4
                }
            }
            tx_tars.push(rx_tar);
        }

        tx_tars
    }
}

#[cfg(test)]
mod tests {
    use crate::ToCesName;
    use super::*;

    #[test]
    fn test_rex_singles() {
        let phrase =
            "{ a => b <= c } { d() + e!(f) g!(h, i) } + { { j -> k -> l } { j -> k } { l <- k } }";
        let rex: Rex = phrase.parse().unwrap();

        assert_eq!(
            rex,
            Rex {
                kinds: vec![
                    RexKind::Sum(RexTree { ids: vec![1, 8] }),
                    RexKind::Product(RexTree { ids: vec![2, 3] }),
                    RexKind::Fat(FatArrowRule {
                        parts: vec![
                            FatArrow {
                                cause:  Polynomial::from("a"),
                                effect: Polynomial::from("b"),
                            },
                            FatArrow {
                                cause:  Polynomial::from("c"),
                                effect: Polynomial::from("b"),
                            }
                        ],
                    }),
                    RexKind::Sum(RexTree { ids: vec![4, 5] }),
                    RexKind::Instance(CesInstance { name: "d".to_ces_name(), args: vec![] }),
                    RexKind::Product(RexTree { ids: vec![6, 7] }),
                    RexKind::Instance(CesInstance {
                        name: "e".to_ces_name(),
                        args: vec!["f".to_string()],
                    }),
                    RexKind::Instance(CesInstance {
                        name: "g".to_ces_name(),
                        args: vec!["h".to_string(), "i".to_string()],
                    }),
                    RexKind::Product(RexTree { ids: vec![9, 10, 11] }),
                    RexKind::Thin(ThinArrowRule {
                        nodes:  NodeList::from(vec!["k"]),
                        cause:  Polynomial::from("j"),
                        effect: Polynomial::from("l"),
                    }),
                    RexKind::Thin(ThinArrowRule {
                        nodes:  NodeList::from(vec!["j"]),
                        cause:  Polynomial::default(),
                        effect: Polynomial::from("k"),
                    }),
                    RexKind::Thin(ThinArrowRule {
                        nodes:  NodeList::from(vec!["l"]),
                        cause:  Polynomial::from("k"),
                        effect: Polynomial::default(),
                    }),
                ],
            }
        );
    }

    #[test]
    fn test_fit_arrow() {
        let phrase = "a => b";
        let rex: Rex = phrase.parse().unwrap();

        assert_eq!(
            rex,
            Rex {
                kinds: vec![RexKind::Fat(FatArrowRule {
                    parts: vec![FatArrow {
                        cause:  Polynomial::from("a"),
                        effect: Polynomial::from("b"),
                    },],
                }),],
            }
        );

        let rex = rex.fit_clone();

        assert_eq!(
            rex,
            Rex {
                kinds: vec![
                    RexKind::Sum(RexTree { ids: vec![1, 2] }),
                    RexKind::Thin(ThinArrowRule {
                        nodes:  NodeList::from(vec!["a"]),
                        cause:  Polynomial::default(),
                        effect: Polynomial::from("b"),
                    }),
                    RexKind::Thin(ThinArrowRule {
                        nodes:  NodeList::from(vec!["b"]),
                        cause:  Polynomial::from("a"),
                        effect: Polynomial::default(),
                    }),
                ],
            }
        );
    }

    #[test]
    fn test_fit_arrow_sequence() {
        let phrase = "a => b => c";
        let rex: Rex = phrase.parse().unwrap();
        let rex = rex.fit_clone();

        assert_eq!(
            rex,
            Rex {
                kinds: vec![
                    RexKind::Sum(RexTree { ids: vec![1, 2, 3] }),
                    RexKind::Thin(ThinArrowRule {
                        nodes:  NodeList::from(vec!["a"]),
                        cause:  Polynomial::default(),
                        effect: Polynomial::from("b"),
                    }),
                    RexKind::Thin(ThinArrowRule {
                        nodes:  NodeList::from(vec!["b"]),
                        cause:  Polynomial::from("a"),
                        effect: Polynomial::from("c"),
                    }),
                    RexKind::Thin(ThinArrowRule {
                        nodes:  NodeList::from(vec!["c"]),
                        cause:  Polynomial::from("b"),
                        effect: Polynomial::default(),
                    }),
                ],
            }
        );
    }

    #[test]
    fn test_fit_fork() {
        let phrase = "a <= b => c";
        let rex: Rex = phrase.parse().unwrap();
        let rex = rex.fit_clone();

        assert_eq!(
            rex,
            Rex {
                kinds: vec![
                    RexKind::Sum(RexTree { ids: vec![1, 2] }),
                    RexKind::Thin(ThinArrowRule {
                        nodes:  NodeList::from(vec!["b"]),
                        cause:  Polynomial::default(),
                        effect: Polynomial::from(vec![vec!["a"], vec!["c"]]),
                    }),
                    RexKind::Thin(ThinArrowRule {
                        nodes:  NodeList::from(vec!["a", "c"]),
                        cause:  Polynomial::from("b"),
                        effect: Polynomial::default(),
                    }),
                ],
            }
        );
    }
}
