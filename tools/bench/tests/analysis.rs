use baauto_bench::*;
use baauto_config::*;
#[test]
fn ring_and_presentmon_csv_parsing_and_missing_fields() {
    let ring="seq,kind,session,action,stage,hook_enter,enqueue,engine_receive,deadline,send_begin,send_end,inserted,expected,queue_depth,config_version,geometry_version,source_seq\n0,2,1,1,1,100,110,130,140,150,160,1,1,0,1,1,1\n2,2,1,1,2,100,110,130,180,200,210,1,1,0,1,1,1\n";
    let r=parse_ring(ring).unwrap();assert_eq!(r.lost,1);let metrics=software_metrics(&r,TickRate(1_000_000)).unwrap();assert_eq!(metrics.hook_us,vec![10.,10.]);assert_eq!(metrics.wake_us,vec![20.,20.]);assert_eq!(metrics.late_us,vec![10.,20.]);assert_eq!(metrics.inject_us,vec![10.,10.]);
    assert!(parse_ring("seq,kind\n1,2\n").is_err());
    let frames=parse_presentmon("Application,CPUStartQPC,MsBetweenPresents,MsUntilDisplayed\n\"a,b.exe\",170,16.7,2.1\na.exe,190,16.6,NA\n").unwrap();assert_eq!(frames.len(),2);assert_eq!(frames[1].display_ms,None);assert!(parse_presentmon("CPUStartQPC\n170\n").is_err());
    assert_eq!(first_frame_after(Ticks(160),&frames),Some(0));assert_eq!(first_frame_after(Ticks(170),&frames),Some(1));assert_eq!(first_frame_after(Ticks(190),&frames),None);
}
#[test]
fn quantiles_require_enough_tail_samples() {
    let small=summarize(&[1.,2.,3.,4.,5.]).unwrap();assert_eq!(small.p50,3.);assert_eq!(small.p95,5.);assert_eq!(small.p99,None);assert_eq!(small.p999,None);
    let many:Vec<_>=(1..=10_000).map(f64::from).collect();let s=summarize(&many).unwrap();assert_eq!(s.p99,Some(9900.));assert_eq!(s.p999,Some(9990.));assert!(summarize(&[f64::NAN]).is_err());
}
#[test]
fn wilson_known_values_and_minimum_attempts() {
    let c=wilson(600,600,0.95).unwrap();assert!((c.lower-0.993638).abs()<0.00001);assert!((c.upper-1.).abs()<1e-12);
    assert_eq!(minimum_zero_failures(0.995,0.95).unwrap(),765,"two-sided Wilson is stricter than the approximate rule of three");
    assert!(wilson(2,1,0.95).is_err());assert!(wilson(0,0,0.95).is_err());
}
#[test]
fn non_monotonic_scan_finds_every_reliable_segment() {
    let samples:Vec<_>=(0..=10).map(|x|ScanPoint{value_ms:f64::from(x),successes:if (3..=5).contains(&x)||(8..=9).contains(&x){1000}else{0},attempts:1000}).collect();
    let ranges=reliable_ranges(&samples,0.99,0.95).unwrap();assert_eq!(ranges,vec![[3.,5.],[8.,9.]]);assert_eq!(refine_boundaries(&samples,0.99,0.95,0.25).unwrap(),vec![2.25,2.5,2.75,5.25,5.5,5.75,7.25,7.5,7.75,9.25,9.5,9.75]);
}
#[test]
fn plan_hash_and_precommitted_gates_control_verification() {
    let p=Plan::example();let hash=p.hash().unwrap();assert_eq!(hash.len(),64);let mut changed=p.clone();changed.min_success=0.9;assert_ne!(hash,changed.hash().unwrap());
    let mut result=TrialEvidence{plan_hash:hash,identity:p.identity.clone(),min_success:p.min_success,confidence:p.confidence,detector_validated:true,detector_error:0.,degraded_flags:0,samples:vec![ScanPoint{value_ms:30.,successes:1000,attempts:1000}],margin_ms:2.,p999_late_ms:Some(1.)};
    assert!(result.verified(&p).unwrap());assert!(!result.verified(&changed).unwrap());result.degraded_flags=1;assert!(!result.verified(&p).unwrap());result.degraded_flags=0;result.detector_validated=false;assert!(!result.verified(&p).unwrap());
}
#[test]
fn detector_confusion_and_synthetic_pipeline() {
    let c=confusion(&[(true,true),(true,false),(false,true),(false,false)]);assert_eq!((c.tp,c.fp,c.fn_,c.tn),(1,1,1,1));assert_eq!(c.error_rate(),Some(0.5));
    let game=baauto_core::sim::FakeGame{accepted_ranges:vec![[3.,5.]],frame_ticks:100};let data:Vec<_>=(0..=10).map(|x|ScanPoint{value_ms:f64::from(x),successes:(0..1000).filter(|phase|game.accepts(f64::from(x),*phase%100)).count() as u64,attempts:1000}).collect();assert_eq!(reliable_ranges(&data,0.99,0.95).unwrap(),vec![[3.,5.]]);
    let report=markdown_scan(&Plan::example(),&data).unwrap();assert!(report.contains("1000")&&report.contains("未驗證"));
}
#[test]
fn trial_phases_are_recorded_and_bounded() {
    let mut rng=PhaseRng::new(42);let values:Vec<_>=(0..10_000).map(|_|rng.next_phase(100)).collect();assert!(values.iter().all(|v|*v<100));let mean=values.iter().sum::<u64>() as f64/values.len() as f64;assert!((mean-49.5).abs()<1.);
}
