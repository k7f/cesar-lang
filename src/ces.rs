use aces::{Content, NodeID};
use crate::{VisBlock, CapacityBlock, MultiplierBlock, InhibitorBlock, Rex};

#[derive(Debug)]
pub struct CesFile {
    blocks: Vec<CesFileBlock>,
}

impl From<Vec<CesFileBlock>> for CesFile {
    fn from(blocks: Vec<CesFileBlock>) -> Self {
        CesFile { blocks }
    }
}

impl Content for CesFile {
    fn get_script(&self) -> Option<&str> {
        None // FIXME
    }

    fn get_name(&self) -> Option<&str> {
        None // FIXME
    }

    fn get_carrier_ids(&self) -> Vec<NodeID> {
        Vec::new()
    }

    fn get_causes_by_id(&self, _id: NodeID) -> Option<&Vec<Vec<NodeID>>> {
        None // FIXME
    }

    fn get_effects_by_id(&self, _id: NodeID) -> Option<&Vec<Vec<NodeID>>> {
        None // FIXME
    }
}

#[derive(Debug)]
pub enum CesFileBlock {
    Imm(ImmediateDef),
    Vis(VisBlock),
    Cap(CapacityBlock),
    Mul(MultiplierBlock),
    Inh(InhibitorBlock),
}

impl From<ImmediateDef> for CesFileBlock {
    fn from(imm: ImmediateDef) -> Self {
        CesFileBlock::Imm(imm)
    }
}

impl From<VisBlock> for CesFileBlock {
    fn from(vis: VisBlock) -> Self {
        CesFileBlock::Vis(vis)
    }
}

impl From<CapacityBlock> for CesFileBlock {
    fn from(cap: CapacityBlock) -> Self {
        CesFileBlock::Cap(cap)
    }
}

impl From<MultiplierBlock> for CesFileBlock {
    fn from(mul: MultiplierBlock) -> Self {
        CesFileBlock::Mul(mul)
    }
}

impl From<InhibitorBlock> for CesFileBlock {
    fn from(inh: InhibitorBlock) -> Self {
        CesFileBlock::Inh(inh)
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub struct CesName(String);

impl From<String> for CesName {
    fn from(name: String) -> Self {
        CesName(name)
    }
}

pub trait ToCesName {
    fn to_ces_name(&self) -> CesName;
}

impl<S: AsRef<str>> ToCesName for S {
    fn to_ces_name(&self) -> CesName {
        self.as_ref().to_string().into()
    }
}

#[derive(Clone, Debug)]
pub struct ImmediateDef {
    name: CesName,
    rex:  Rex,
}

impl ImmediateDef {
    pub fn new(name: CesName, rex: Rex) -> Self {
        ImmediateDef { name, rex }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CesInstance {
    pub(crate) name: CesName,
    pub(crate) args: Vec<String>,
}

impl CesInstance {
    pub(crate) fn new(name: CesName) -> Self {
        CesInstance { name, args: Vec::new() }
    }

    pub(crate) fn with_args(mut self, mut args: Vec<String>) -> Self {
        self.args.append(&mut args);
        self
    }
}
