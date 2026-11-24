use dicom_rust::net::dimse::{self, CStoreRequestContext};
use dicom_rust::net::ul::{self, Ctx};
use dicom_rust::object::{self, Tag};
use std::error::Error;
use std::sync::Arc;
use std::thread;
const VERIFICATION: &str = "1.2.840.10008.1.1";
const IMPLICIT_LE: &str = "1.2.840.10008.1.2";
const EXPLICIT_LE: &str = "1.2.840.10008.1.2.1";
fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/examples/data/ct_small.dcm").to_string()
    });
    let file = object::open_file(&path).map_err(|(_, e)| e)?;
    let sop_class = file
        .get_uid(Tag::new(0x0008, 0x0016))
        .ok_or("the file has no SOP Class UID")?;
    let listener = ul::listen(ul::ListenOptions::default())?;
    let address = listener
        .local_addr()
        .ok_or("the listener has no address")?
        .to_string();
    let accept = ul::AcceptOptions {
        ae_title: "STORE_SCP".into(),
        supported_abstract_syntaxes: vec![VERIFICATION.into(), sop_class.clone()],
        supported_transfer_syntaxes: vec![EXPLICIT_LE.into(), IMPLICIT_LE.into()],
        ..Default::default()
    };
    let handler: dimse::CStoreHandler = Arc::new(|_: &Ctx, req: &CStoreRequestContext| {
        let name = req
            .data_set
            .as_ref()
            .and_then(|ds| ds.get_string(Tag::new(0x0010, 0x0010)))
            .unwrap_or_default();
        println!(
            "SCP: C-STORE of {} (patient {name})",
            req.request.affected_sop_instance_uid
        );
        (0x0000, None)
    });
    let scp_options = dimse::AssociationScpOptions {
        storage: dimse::StorageScpOptions {
            store_handler: Some(handler),
            ..Default::default()
        },
        ..Default::default()
    };
    let scp = thread::spawn(move || -> Result<(), String> {
        let association = listener
            .accept_association(accept)
            .map_err(|e| e.to_string())?;
        let served = dimse::serve_association(None, &association, &scp_options);
        association.close();
        served.map_err(|e| e.to_string())
    });
    let dial = ul::DialOptions {
        called_ae_title: "STORE_SCP".into(),
        calling_ae_title: "STORE_SCU".into(),
        contexts: vec![
            ul::PresentationContext::new(VERIFICATION, &[IMPLICIT_LE]),
            ul::PresentationContext::new(&sop_class, &[EXPLICIT_LE, IMPLICIT_LE]),
        ],
        ..Default::default()
    };
    let association = ul::dial(&address, dial)?;
    let verification = dimse::accepted_verification_context(&association)
        .ok_or("Verification was not accepted")?;
    let echo = dimse::send_c_echo(None, &association, verification.id, 1)?;
    println!("SCU: C-ECHO status 0x{:04X}", echo.status);
    let mut client = dimse::StoreClient::new(&association);
    let stored = client
        .store_with_options(None, &file.dataset, Default::default())
        .map_err(|(_, e)| e)?;
    let status = stored.response.map_or(0xFFFF, |r| r.status);
    println!("SCU: C-STORE status 0x{status:04X}");
    association.release(None)?;
    association.close();
    scp.join().map_err(|_| "the SCP thread panicked")??;
    Ok(())
}
