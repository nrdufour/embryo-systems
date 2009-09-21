%% This is the application resource file (.app file) for the 'base'
%% application.
{application, embryosys,
	[{description, "Embryo Systems"},
	 {vsn, "0.1"},
	 {modules, [embryosys_app, embryosys_supervisor, adt_executor,
		    adtm_family, adtm_property, adtm_link, adtm_entity]},
	 {registered, [adt_executor, adtm_family, adtm_property,
		       adtm_link, adtm_entity, embryosys_supervisor]},
	 {applications, [kernel,stdlib]},
	 {mod, {embryosys_app, []}},
	 {start_phases, []}
]}.

