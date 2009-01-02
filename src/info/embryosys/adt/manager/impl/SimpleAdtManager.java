/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 */
package info.embryosys.adt.manager.impl;

import info.embryosys.adt.core.Adt;
import info.embryosys.adt.core.AdtFactory;
import info.embryosys.adt.core.AdtId;
import info.embryosys.adt.core.AdtState;
import info.embryosys.adt.manager.AdtManager;
import info.embryosys.adt.manager.Workspace;
import info.embryosys.adt.request.AdtRequest;
import info.embryosys.adt.storage.Storage;
import info.embryosys.adt.storage.impl.DummyStorage;

/**
 * @author nrdufour
 * 
 */
public class SimpleAdtManager implements AdtManager {

	private Workspace workspace;

	/**
	 * @return the workspace
	 */
	public Workspace getWorkspace() {
		return workspace;
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see info.embryosys.adt.manager.AdtManager#init()
	 */
	public void init() {
		// FIXME for now it's hardcoded .. I will find a way to configure that
		// through a conf file

		this.workspace = new Workspace("default", new DummyStorage());
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see
	 * info.embryosys.adt.manager.AdtManager#processRequest(info.embryosys.adt
	 * .manager.AdtRequest)
	 */
	public void processRequest(final AdtRequest request) {
		Storage storage = workspace.getStorage();

		System.out.println("Processing request: " + request);

		// FIXME must check the existence of the parents

		switch (request.getOperation()) {
		case CREATE:
			create(storage, request);
			break;
		case HIBERN:
		case AWAKE:
		case DESTROY:
		case RESUR:
			hadr(storage, request);
			break;
		case PURGE:
			purge(storage, request);
			break;
		case RENAME:
			rename(storage, request);
			break;
		default:
		}
	}

	/**
	 * @param storage
	 * @param request
	 */
	private void purge(Storage storage, AdtRequest request) {
		// TODO Auto-generated method stub

	}

	/**
	 * @param storage
	 * @param request
	 */
	private void rename(Storage storage, AdtRequest request) {
		// TODO Auto-generated method stub

	}

	/**
	 * @param storage
	 * @param request
	 */
	private void hadr(Storage storage, AdtRequest request) {
		String name = request.getName();

		AdtId adtId = storage.find(request.getType(), name);
		if (adtId == null) {
			throw new RuntimeException("The ADT [" + name + "] doesn't exist !");
		}

		Adt adt = storage.load(adtId);

		// FIXME need to check that the CURRENT state is compatible here

		AdtState state = null;
		switch (request.getOperation()) {
		case HIBERN:
			state = AdtState.FROZEN;
			break;
		case AWAKE:
			state = AdtState.ALIVE;
			break;
		case DESTROY:
			state = AdtState.DESTROYED;
			break;
		case RESUR:
			state = AdtState.ALIVE;
			break;
		default:
		}

		if (state == null) {
			throw new RuntimeException(
					"Wrong state/Wrong Operation during HADR operation !");
		}

		adt.setState(state);

		storage.store(adt);
	}

	public void create(final Storage storage, final AdtRequest request) {
		// FIXME must check first if that adt exists or not.

		Adt adt = AdtFactory.createNewAdt(request.getType(), request.getName());

		adt.setState(AdtState.ALIVE);

		storage.store(adt);
	}
}
